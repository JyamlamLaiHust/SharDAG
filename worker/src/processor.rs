use crate::csmsg_store::AppendedType;
use crate::csmsg_store::CSMsgStore;
use crate::utils::shuffle_node_id_list;
use crate::worker::SerializedBatchDigestMessage;
use crate::worker::WorkerMessage;
use config::NodeId;
use config::WorkerId;
use crypto::Digest;
use ed25519_dalek::Digest as _;
use ed25519_dalek::Sha512;
use log::debug;
use log::info;
use primary::WorkerPrimaryMessage;
use std::convert::TryInto;
use store::Store;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::error::ExecutionResult;


// #[cfg(test)]
// #[path = "tests/processor_tests.rs"]
// pub mod processor_tests;

/// Indicates a serialized `WorkerMessage::Batch` message.
// 表示序列化后的 `WorkerMessage::Batch` 消息
pub type SerializedBatchMessage = Vec<u8>;

/// Hashes and stores batches, it then outputs the batch's digest.
// 对批次进行哈希，并存储批次信息，最后输出批次的摘要
pub struct Processor {
  id: WorkerId,
  nodeid: NodeId,
  is_malicious: bool,
  shard_size: usize,
  store: Store,
  rx_batch: Receiver<SerializedBatchMessage>,
  tx_digest: Sender<SerializedBatchDigestMessage>,
  own_digest: bool,
  csmsg_store: CSMsgStore,
  cs_rev_nums: usize, // number of cross-shard receivers 
}

impl Processor {
    pub fn spawn(
        // Our worker's id.
        id: WorkerId,
        nodeid: NodeId,
        is_malicious: bool,
        shard_size: usize,
        // The persistent storage.
        store: Store,
        // Input channel to receive batches.
        rx_batch: Receiver<SerializedBatchMessage>,
        // Output channel to send out batches' digests.
        tx_digest: Sender<SerializedBatchDigestMessage>,
        // Whether we are processing our own batches or the batches of other nodes.
        own_digest: bool,
        csmsg_store: CSMsgStore,
        cs_rev_nums: usize,
    ) {

      tokio::spawn(async move {
        Self {
            id,
            nodeid,
            is_malicious,
            shard_size,
            store,
            rx_batch,
            tx_digest,
            own_digest,
            csmsg_store,
            cs_rev_nums,
        }
        .run()
        .await;
      });
    }

    async fn run(&mut self) {
      while let Some(batch) = self.rx_batch.recv().await {
          // Hash the batch.
          let digest = Digest(Sha512::digest(&batch).as_slice()[..32].try_into().unwrap());

          debug!(
            "Store the batch: {:?}",
            digest,
          );

          // Store the batch.
          self.store.write(digest.to_vec(), batch.clone()).await;

          // TODO create a new future
          // monitor the appending of csmsg from otherBatch
          if !self.own_digest && !self.is_malicious {
            let _ = self.monitor_csmsg_appending(batch).await;
          }

          // Deliver the batch's digest.
          let message = match self.own_digest {
              true => WorkerPrimaryMessage::OurBatch(digest, self.id),
              false => WorkerPrimaryMessage::OthersBatch(digest, self.id),
          };
          let message = bincode::serialize(&message)
              .expect("Failed to serialize our own worker-primary message");
          self.tx_digest
              .send(message)
              .await
              .expect("Failed to send digest");
      }
    }
    // 监控 CSMsg 的追加，并根据验证结果更新存储
    async fn monitor_csmsg_appending(&mut self, batch:SerializedBatchMessage) -> ExecutionResult<bool> {
        // deserialized batch
        let msg = bincode::deserialize(&batch)?;
        // range tx_list tranferTX---leader built
        match msg {
            WorkerMessage::Batch(trbatch) => {
                for tx in trbatch.tx_list.iter() {
                    match tx.get_csmsg_id() {
                      None => {}, // not a csmsg
                      Some(csmsg_id) => {
                        // verify the threshold sig and get tx_hash
                        let tx_hash = tx.verify_cs_proof();
                        match tx_hash {
                          None => {
                            info!("an invalid csmsg is appended!");
                          },
                          Some(tx_hash) => {// valid csmsg
                            // mark this csmsg has been appended regardless of whether the node is a receiver or not
                            // let _ = self.csmsg_store.update_appended(csmsg_id, AppendedType::Remote).await;

                            let candi_node_id_list = shuffle_node_id_list(self.shard_size, &tx_hash);
                            let csmsg_recvs = &candi_node_id_list[0..self.cs_rev_nums];
                            if csmsg_recvs.contains(&(self.nodeid as usize)) {
                              // this node is the csmsg receiver
                              let _ = self.csmsg_store.update_appended(csmsg_id, AppendedType::Remote).await;
                            }
                          }        
                        }    
                      }                        
                    }                
                } // end of for each tx
            },
            _ => {}
        }
        Ok(true)
    }
}
