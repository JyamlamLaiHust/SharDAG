use std::collections::HashMap;
use config::WorkerId;
use crypto::Digest;
use primary::Header;
use tokio::sync::mpsc::{Receiver, Sender};
use log::{info, warn, debug};
use store::Store;
use crate::{error::ExecutionResult, worker::WorkerMessage, batch_fetcher::MissingBatchFetcher};
use crate::worker::{ConversionMessage, SynchronizationMessage};
use crate::batch_maker::Batch;

pub struct TxConvertor {
    store: Store,
    rx_process: Receiver<ConversionMessage>,
    tx_execution: Sender<SynchronizationMessage>,
    missing_batch_fetcher: MissingBatchFetcher,
}

impl TxConvertor {
    pub fn spawn(
      store: Store,
      rx_process: Receiver<ConversionMessage>,
      tx_execution: Sender<SynchronizationMessage>,
      missing_batch_fetcher: MissingBatchFetcher,
    ) {    
        tokio::spawn(async move {
          Self {
            store,
            rx_process,
            tx_execution,
            missing_batch_fetcher,
          }
          .run()
          .await;
      });
    }
      /// Main loop listening to the messages.
      async fn run(&mut self) {
        info!(
          "TxConversion is running!"
        );

        while let Some(ConversionMessage{ height, header/*digest, payload*/ }) = self.rx_process.recv().await {
            debug!(
              "Receiving conversion msg for height: {}, header {}",
              height, header,
            );

            let (mut batch_list, missing) = self.try_fetch_payload(&header).await.unwrap();
            if !missing.is_empty() { // some batches are missing, fetch them first
              self.fetch_missing_batch(&header, missing).await;
              (batch_list, _) = self.try_fetch_payload(&header).await.unwrap();
            }
            if header.payload.len() != batch_list.len() {
              // warn!(
              //   "After conversion: height: {}, header: {}, total batch num: {}, loaded batch num: {}",
              //   height, header, header.payload.len(), batch_list.len()
              // );
              warn!("[height: {}] load batches of header {:?} failed! total batch num: {}, loaded batch num: {}", height, header, header.payload.len(), batch_list.len());
            }

            // send batch_list to executor
            let message = SynchronizationMessage {height, /*digest*/header, batch_list};
            self.tx_execution
                .send(message)
                .await
                .expect("Failed to send new block to Executor");
        }
      }

      pub async fn try_fetch_payload(&mut self, header: &Header) -> ExecutionResult<(Vec<Batch>, HashMap<Digest, WorkerId>)> {
        let mut batch_list: Vec<Batch> = Vec::new();
        let mut missing: HashMap<Digest, WorkerId> = HashMap::new();
        for (digest, worker_id) in header.payload.iter() {
          match self.store.read(digest.to_vec()).await? {
            Some(serialized) => {
              let msg = bincode::deserialize(&serialized)?;
              match msg {
                  WorkerMessage::Batch(batch) => {
                      batch_list.push(batch);
                  },
                  _ => {}                       
              }
            },
            None => {
              info!("batch from author {:?} is missing! batch.digest: {:?}", header.author, digest);
              missing.insert(digest.clone(), *worker_id);
            }
          };
        }
        Ok((batch_list, missing))
      }

      pub async fn fetch_missing_batch(&mut self, header: &Header, missing: HashMap<Digest, WorkerId> ) {
        // fetch missing batches (blocking)
        info!("Synching the payload of header: {}", header);
        info!("Missing batches: {:?}", missing);
        let target = header.author;
        let mut requires_sync = HashMap::new(); // Hashmap<worker_id, Vec<batch.digest>>.
        for (digest, worker_id) in missing.into_iter() { // for each batch
            requires_sync.entry(worker_id).or_insert_with(Vec::new).push(digest);
        }
        // there is only one worker per node in SharDAG
        for (_, digests) in requires_sync {
          // fetch missing batches from header.author.worker_id
          let _ = self.missing_batch_fetcher.fetch_missing_batches(digests, target).await;
        }
      }
}



