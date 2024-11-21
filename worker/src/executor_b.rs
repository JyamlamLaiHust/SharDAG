use config::ShardId;
use primary::Header;
use tokio::sync::mpsc::{Receiver, Sender};
use log::{info, debug, warn};
use crate::csmsg_store::CSMsgStore;
use crate::executor_s::ExecutionState;
use crate::{Frame, Account2Shard, Address, StateTransition};
use crate::batch_maker::Batch;
use crate::worker::{SynchronizationMessage, SendCSMessage};
use crate::messages::{GeneralTransaction, Height};
extern crate csv;
extern crate serde_derive;


pub struct BExecutor {
    // node config
    _shard_id: ShardId,

    // channel
    rx_process_txs: Receiver<SynchronizationMessage>,
    tx_csmsg: Sender<SendCSMessage>,

    // state store
    state_transition: StateTransition,
    _acc2shard: Box<dyn Account2Shard + Send>,
    csmsg_store: CSMsgStore,

    // statistical info
    total_general_txs: u32,
    total_external_txs: u32,
    total_cross_shard_txs: u32,
    total_commit_txs: u32,
    total_aborted_txs: u32,
}



impl BExecutor {
    pub fn spawn(
      // node config
      _shard_id: ShardId,

      rx_process_txs: Receiver<SynchronizationMessage>,
      tx_csmsg: Sender<SendCSMessage>,

      // state store
      state_transition: StateTransition,
      _acc2shard: Box<dyn Account2Shard + Send>,
      csmsg_store: CSMsgStore,
    ) {
        
        tokio::spawn(async move {
          Self {
            _shard_id,
            state_transition,
            _acc2shard,
            csmsg_store,
            rx_process_txs,
            tx_csmsg,
            
            total_general_txs: 0,
            total_external_txs: 0,
            total_commit_txs: 0,
            total_cross_shard_txs: 0,
            total_aborted_txs: 0,
          }
          .run()
          .await;
      });
    }

    /// Main loop listening to the messages.
    async fn run(&mut self) {

      info!("BExecutor is running!");
      
      while let Some(SynchronizationMessage{height, header, batch_list}) = self.rx_process_txs.recv().await {
        debug!(
          "[height: {}][header: {}] Receiving TxBlock msg",
          height, header
        );
        self.process_tx_block(height, header, batch_list).await;        
      }
    }


    // verify tx before execution, ignore invalid tx or redundant csmsg tx
    // return (is_csmsg, is_valid)
    async fn verify_tx(&mut self, tx: &GeneralTransaction) -> (Option<String>, bool) {
      match tx.get_csmsg_id() {
        None => {
          (None, true) // TODO: we assume that intra-shard tx is identical
        }, // not a csmsg
        Some(csmsg_id) => {
          // verify the threshold sig and get tx_hash
          let tx_hash = tx.verify_cs_proof();
          match tx_hash {
            None => { // invalid csmsg
              (Some(csmsg_id), false)
            },
            Some(_) => {// valid csmsg, may be redundant
              let can_executed = self.csmsg_store.can_executed(csmsg_id.clone()).await.unwrap();
              if can_executed {
                (Some(csmsg_id), true)
              } else {
                (Some(csmsg_id), false)
              }
            }                            
          }    
        }                        
      }  
    }


    async fn process_tx_block(&mut self, height: Height, header: Header, batch_list: Vec<Batch>) {

        let mut cur_general_txs = 0;

        let mut digest_iterator = header.payload.iter();
        for batch in batch_list {

          let digest = digest_iterator.next().unwrap().0;
          debug!(
            "[height: {}][header: {}] process batch: {:?}",
            height, header, digest
          ); 

          for tx in batch.tx_list {
            // verify tx before execution, ignore invalid tx or redundant csmsg tx
            let (_, is_valid) = self.verify_tx(&tx).await;
            if !is_valid {
              continue;
            }
            
            self.total_general_txs += 1;
            cur_general_txs += 1;

            match tx {
              GeneralTransaction::TransferTx(transfer_tx) => {
                debug!(
                  "[height: {}] process transfer tx: {:?}",
                  height, transfer_tx.counter 
                );

                let exec_state = self.exec_tx(&transfer_tx.payload, transfer_tx.step, &transfer_tx.receiver, &transfer_tx.final_receiver).await;                

                match exec_state {
                  ExecutionState::Commit => {
                    debug!(
                      "[height: {}] commit tx: {:?}",
                      height, transfer_tx.get_digest() 
                    );
                    
                    self.total_external_txs += 1;
                    self.total_cross_shard_txs += transfer_tx.count_cs_tx();
                    self.total_commit_txs += 1;
                    // output sample info
                    if transfer_tx.sample == 0 { // sample tx
                      info!(
                        "Successfully execute sample tx {} in batch",
                          transfer_tx.counter
                      );
                    }
                  },
                  ExecutionState::Relay => {
                    // send tx1 to broker client
                    debug!(
                      "[height: {}] broker tx1: {:?}",
                      height, transfer_tx
                    );

                    // send csmsg to target shard
                    let message = SendCSMessage{height, target_shard: ShardId::MAX, tx: GeneralTransaction::TransferTx(transfer_tx)};
                    self.tx_csmsg
                        .send(message)
                        .await
                        .expect("Failed to send new block to ADSynchronizer");
                  },
                  ExecutionState::Abort => {
                      self.total_external_txs += 1;
                      self.total_cross_shard_txs += transfer_tx.count_cs_tx();                      
                      self.total_aborted_txs += 1;
                      warn!(
                        "[height: {}] insufficient balance, fail to execute tx: {:?}",
                        height, transfer_tx
                      );
                  }
                }
              },
              GeneralTransaction::AggTx(_) => {}
            }
          }// end of for 
        }

        // commit updated states
        let _ = self.state_transition.store.root().await;

        if cur_general_txs != 0 {
          info!(
            "[height: {}] total_general_txs: {}, total_external_txs: {}, total_cross_shard_txs: {}, total_commit_txs: {}, total_aborted_txs: {}",
              height, self.total_general_txs, self.total_external_txs, self.total_cross_shard_txs, self.total_commit_txs, self.total_aborted_txs
          );
        }
    }
    

    // execute the step-th frame in payload
    async fn exec_tx (
      &mut self, 
      payload: &Vec<Frame>, step: usize,
      receiver: &Address,
      final_receiver: &Option<Address>,
    ) -> ExecutionState {

      // get the latest states of the involved accs
      let frame = payload.get(step).unwrap();
      let mut latest_states = self.state_transition.get_latest_states(&frame.rwset).await;
      debug!{"latest states: {:?}", latest_states};

      // balance check
      let mut pass_check = true;
      for rwset in &frame.rwset {
        let acc = latest_states.get_mut(&rwset.addr).unwrap();
        acc.balance += rwset.value;

        if rwset.value < 0.0 { // deduction
          if acc.balance < 0.0 { // the balance is negative after execution
            pass_check = false;
            break;
          }
          acc.nonce +=1;          
        }
      }

      if !pass_check {
        return ExecutionState::Abort;
      }
      // pass check
      self.state_transition.apply_new_states(latest_states).await;

      // check if the transaction needs to be relayed
      if self._is_broker_tx1(receiver, final_receiver) {
        ExecutionState::Relay
      } else {
        ExecutionState::Commit   
      }
    }
   
    fn _is_broker_tx1(&self, receiver: &Address, final_receiver: &Option<Address>) -> bool{
      match final_receiver {
        None => {false}
        Some(final_recv) => {
          *receiver != *final_recv
        }
      }   
    }
}