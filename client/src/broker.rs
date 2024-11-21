use std::sync::Arc;
use config::ShardId;
use log::{info, debug, warn};
use store::StoreError;
use worker::{Transaction, RWSet, Account2Shard, Account2ShardType, Account2ShardHash, Account2ShardGraph, Frame, CoreTx, Address};
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::oneshot;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::broker_manager::BrokerManager;

pub type StoreResult<T> = Result<T, StoreError>;

pub enum BrokerCommand {
  ConvertTx(CoreTx, oneshot::Sender<StoreResult<(Transaction, ShardId)>>), 
  ProcessTx1(Transaction, oneshot::Sender<StoreResult<(Transaction, ShardId)>>),
}


// Manage brokers and handle cross-shard tx
#[derive(Clone)]
pub struct Broker {
    channel: Sender<BrokerCommand>,
}


impl Broker {
  pub fn new(
    acc_shard_type: Account2ShardType,
    shardnum: usize,
    acc2shard_file: String,
    brokers_file: String,
    epoch: usize,
  ) -> Self {

    info!("Create a broker!");
    // crate acc_shard according to specified sharding policy
    let acc2shard: Arc<dyn Account2Shard + Send + Sync>;
    match acc_shard_type {
      Account2ShardType::HashPolicy => {
        info!("Account2Shard: HashPolicy");
        acc2shard = Arc::new(Account2ShardHash::new(shardnum));
      },
      Account2ShardType::GraphPolicy => {
        info!("Account2Shard: GraphPolicy");
        acc2shard = Arc::new(Account2ShardGraph::new(shardnum, &acc2shard_file));
      }
    }

    // init broker addresses
    let mut broker_manager = BrokerManager::new(brokers_file, epoch);

    let (tx, mut rx) = channel(1000);
    tokio::spawn(async move {
      while let Some(command) = rx.recv().await {
          match command {
              // when this func is called, this csmsg has been validated
              BrokerCommand::ConvertTx(core_tx, sender) => {
                let target_shard: ShardId;

                let core_tx_copy = core_tx.clone();

                let mut original_sender: Option<Address> = None;
                let mut final_receiver: Option<Address> = None;

                let s = core_tx.sender;
                let mut r = core_tx.receiver;
                let amount = core_tx.amount;
              
                let sender_s  = acc2shard.get_shard(&s);
                let receiver_s  = acc2shard.get_shard(&r);

                let mut involved_shard_num = 1;
                // if sender_s != receiver_s { // orignal cs tx
                //   involved_shard_num = 2;
                // }

                if sender_s != receiver_s && !broker_manager.is_broker(&s) && !broker_manager.is_broker(&r) {
                  // cs-tx handled by broker
                  involved_shard_num = 2;

                  target_shard = sender_s;
                  // generate tx1
                  original_sender = Some(s.clone());
                  final_receiver = Some(r.clone());

                  let broker = broker_manager.get_broker();
                  r = broker.clone();
                  debug!("handled by broker: {:?}", core_tx.counter);
                  broker_manager.add_tx1(core_tx_copy, core_tx.counter, broker);
                } else { 
                  if broker_manager.is_broker(&s) {
                    // send this tx to reciever's shard
                    target_shard = receiver_s;
                    debug!("sender is a broker");
                  } else {
                    target_shard = sender_s;
                  }
                }
                // assemble tx
                let tx = assemble_tx(
                  s, r, amount, 
                  core_tx.sample, core_tx.counter, target_shard, involved_shard_num,
                  original_sender, final_receiver);
                let _ = sender.send(Ok((tx, target_shard))
              );
              }
              BrokerCommand::ProcessTx1(tx1, sender) => {
                debug!("process tx1: {:?}", tx1.counter);
                let tx_counter = tx1.counter;
                let tx1_msg = broker_manager.delete_tx1(tx_counter);
                match tx1_msg {
                  None => {
                    // TODO: fix BUG
                    warn!("tx1 confirms failure! tx1 {:?} does not exist!", tx1.counter);
                  },
                  Some(tx1_msg) => {
                      debug!("removed tx1: {:?}", tx_counter);
                      // generate tx2
                      let target_shard = acc2shard.get_shard(&tx1_msg.core_tx.receiver);
                      
                      // generate tx1
                      let original_sender = Some(tx1_msg.core_tx.sender);
                      let final_receiver = Some(tx1_msg.core_tx.receiver.clone());

                      let broker = broker_manager.get_broker(); // in tx2，broker as sender

                      // assemble tx
                      let tx = assemble_tx(
                        broker, tx1_msg.core_tx.receiver, tx1_msg.core_tx.amount, 
                        tx1_msg.core_tx.sample, tx1_msg.core_tx.counter, target_shard, 2,
                        original_sender, final_receiver);
                      let _ = sender.send(Ok((tx, target_shard)));
                  }
                }

              }
          }
      }
  });
  Self { channel: tx }
  }

  pub async fn convert_tx(&mut self, core_tx: CoreTx) -> StoreResult<(Transaction, ShardId)> {
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(BrokerCommand::ConvertTx(core_tx, sender)).await {
        panic!("Failed to send ConvertTx command to BrokerStore: {}", e);
    }
    receiver
        .await
        .expect("Failed to receive reply to ConvertTx command from BrokerStore")
  }

  pub async fn process_tx1(&mut self, tx1: Transaction) -> StoreResult<(Transaction, ShardId)> {
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(BrokerCommand::ProcessTx1(tx1, sender)).await {
        panic!("Failed to send ProcessTx1 command to BrokerStore: {}", e);
    }
    receiver
        .await
        .expect("Failed to receive reply to ProcessTx1 command from BrokerStore")
  }
}




pub fn assemble_tx(
  sender: Address,
  receiver: Address,
  amount: f64,
  tx_sample: u8,
  tx_counter: u64,
  target_shard: ShardId,
  involved_shard_num: usize,
  original_sender: Option<Address>,
  final_receiver: Option<Address>,
) -> Transaction {
    let payload_len = 2;
    let mut rwsets: Vec<RWSet> = Vec::new();
    rwsets.push(RWSet { addr: sender.clone(), value: amount * -1.0 });
    rwsets.push(RWSet { addr: receiver.clone(), value: amount});
    let payload: Vec<Frame> = vec![Frame { shardid: target_shard, rwset: rwsets }];
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros();

    Transaction::new(sender, receiver, amount, payload, payload_len, involved_shard_num, tx_sample, tx_counter, timestamp, original_sender, final_receiver)
}