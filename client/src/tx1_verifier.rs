use config::{Stake, Committees};
use crypto::{assemble_intact_ts_partial, PublicKey, verify_ts_sig};
use tokio::sync::mpsc::{Receiver, Sender};
use worker::{CSMsgStore, Transaction};
use worker::{GeneralTransaction, CSMsg};
use log::{info, debug, warn};


pub struct Tx1Verifier {
    rx_cross_shard_msg: Receiver<CSMsg>,
    tx_process_tx1: Sender<Transaction>,
    all_committees: Committees,

    vote_threshold: Stake,
    shard_size: usize,
    csmsg_store: CSMsgStore,
}

impl Tx1Verifier {
  pub fn spawn(
      rx_cross_shard_msg: Receiver<CSMsg>,
      tx_process_tx1: Sender<Transaction>,
      vote_threshold: Stake,
      shard_size: usize,
      all_committees: Committees,
      csmsg_store: CSMsgStore,
    ) {
      tokio::spawn(async move {
        Self {
          rx_cross_shard_msg,
          tx_process_tx1,
          all_committees,
          vote_threshold,// f+1
          shard_size,
          csmsg_store,
        }
        .run()
        .await;
      });
    }

    /// Main loop listening to the messages.
    async fn run(&mut self) {
      info!(
        "Tx1Verifier is running!"
      );

      while let Some(cs_msg) = self.rx_cross_shard_msg.recv().await {
        debug!(
          "Receiving tx1, csmsg_seq: {:?}",
          cs_msg.csmsg_sequence,
        );

        if cs_msg.verify(&self.all_committees) {
          self.process_msg(cs_msg).await;
        } else {
          warn!(
            "Threshold signature verification failed",
          );
        }
      }
    }

    async fn process_msg(
      &mut self,
      csmsg: CSMsg,
    ) {
        // get the msgId of the csmsg
        let msg_id = format!("[{}-{}]", csmsg.source_shard, csmsg.csmsg_sequence);
        debug!("process csmsg: {}, counter: {:?}", msg_id, csmsg.get_counter().await);

        // try to add this csmsg sig to csmsg_store
        let (is_added, thres_sig_list) = self.csmsg_store.add_csmsg_sig(msg_id.clone(), csmsg.thres_sig).await.unwrap();
        debug!("res of add_csmsg_sig: {:?}, {:?}", is_added, thres_sig_list);
        if is_added {
          if thres_sig_list.len() != 0 { // reach vote_threshold, 
              // assemble intact ts partial and verify ts
              let inner_tx_hash = csmsg.inner_tx_hash;
              let intact_sig = assemble_intact_ts_partial(thres_sig_list, PublicKey::default(), &inner_tx_hash, self.vote_threshold, self.shard_size);

              if verify_ts_sig(PublicKey::default(), &inner_tx_hash, &intact_sig) {
                // pass verification
                let mut inner_tx = csmsg.tx;
                // update intact_sig of inner_tx
                inner_tx.set_thres_sig(intact_sig, csmsg.source_shard);

                match inner_tx {
                  GeneralTransaction::AggTx(_) => {} // ingore
                  GeneralTransaction::TransferTx(tx1) => {
                    debug!("sends tx1 {:?} to Tx1Proseccor!", tx1.counter);
                    self
                      .tx_process_tx1
                      .send(tx1)
                      .await
                      .expect("Failed to send cs msg");
                  }
                }
              } else {
                warn!("verify threshold signature failed");
              }
          }
        } else { // // this csmsg has been validated, just ignore it
          debug!("ignore this tx1_csmsg: {:?}", msg_id);
        }
    }
}
