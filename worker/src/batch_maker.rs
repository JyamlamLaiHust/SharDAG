use crate::quorum_waiter::QuorumWaiterMessage;
use crate::worker::WorkerMessage;
use crate::messages::GeneralTransaction;
use bytes::Bytes;
// #[cfg(feature = "benchmark")]
use crypto::Digest;
use crypto::PublicKey;
// #[cfg(feature = "benchmark")]
use ed25519_dalek::{Digest as _, Sha512};
// #[cfg(feature = "benchmark")]
use log::info;
use network::ReliableSender;
// #[cfg(feature = "benchmark")]
use std::convert::TryInto as _;
use std::net::SocketAddr;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{sleep, Duration, Instant};
use serde::{Deserialize, Serialize};


// #[cfg(test)]
// #[path = "tests/batch_maker_tests.rs"]
// pub mod batch_maker_tests;

pub type GeneralTxList = Vec<GeneralTransaction>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Batch {
  pub external_tx_nums: usize, // just for test storage overhead
  pub tx_list: GeneralTxList,
}


/// Assemble clients transactions into batches.
pub struct BatchMaker {
    /// The preferred batch size (in bytes).
    batch_size: usize,
    /// The maximum delay after which to seal the batch (in ms).
    max_batch_delay: u64,
    /// Channel to receive transactions from the network.
    rx_transaction: Receiver<GeneralTransaction>,
    /// Output channel to deliver sealed batches to the `QuorumWaiter`.
    tx_message: Sender<QuorumWaiterMessage>,
    /// The network addresses of the other workers that share our worker id.
    workers_addresses: Vec<(PublicKey, SocketAddr)>,
    
    /// Holds the current batch.
    current_batch: GeneralTxList,
    /// Holds the size of the current batch (in bytes).
    current_batch_size: usize,
    /// newly-added
    current_batch_external_txs: usize,  // number of external transactions in the current batch
    current_batch_txs: usize, // number of general transactions in the current batch
    
    total_packaged_external_txs: usize,
    
    /// A network sender to broadcast the batches to the other workers.
    network: ReliableSender,
}

impl BatchMaker {
    pub fn spawn(
        batch_size: usize,
        max_batch_delay: u64,
        rx_transaction: Receiver<GeneralTransaction>,
        tx_message: Sender<QuorumWaiterMessage>,
        workers_addresses: Vec<(PublicKey, SocketAddr)>,
    ) {
        tokio::spawn(async move {
            Self {
                batch_size,
                max_batch_delay,
                rx_transaction,
                tx_message,
                workers_addresses,
                current_batch: GeneralTxList::with_capacity(batch_size * 2),
                current_batch_size: 0,
                current_batch_external_txs: 0,
                current_batch_txs: 0,
                total_packaged_external_txs: 0,         
                network: ReliableSender::new(),
            }
            .run()
            .await;
        });
    }

    /// Main loop receiving incoming transactions and creating batches.
    async fn run(&mut self) {
        let timer = sleep(Duration::from_millis(self.max_batch_delay));
        tokio::pin!(timer);

        loop {
            tokio::select! {
                // Assemble client transactions into batches of preset size.
                Some(transaction) = self.rx_transaction.recv() => {
                    self.current_batch_external_txs += transaction.count_packaged_external_tx() as usize;
                    self.current_batch_txs += 1;
                    self.current_batch_size += transaction.len();
                    self.current_batch.push(transaction);

                    if self.current_batch_size >= self.batch_size {
                        self.seal().await;
                        timer.as_mut().reset(Instant::now() + Duration::from_millis(self.max_batch_delay));
                    }
                },

                // If the timer triggers, seal the batch even if it contains few transactions.
                () = &mut timer => {
                    if !self.current_batch.is_empty() {
                        info!("time out, create new batch");
                        self.seal().await;
                    }
                    timer.as_mut().reset(Instant::now() + Duration::from_millis(self.max_batch_delay));
                }
            }

            // Give the change to schedule other tasks.
            tokio::task::yield_now().await;
        }
    }

    /// Seal and broadcast the current batch.
    async fn seal(&mut self) {

        // Serialize the batch.
        let packaged_batch_external_txs = self.current_batch_external_txs;
        let packaged_batch_size = self.current_batch_size;
        self.current_batch_txs = 0;
        self.current_batch_size = 0;
        self.current_batch_external_txs = 0;


        // assemble batch
        let batch_tx: Vec<_> = self.current_batch.drain(..).collect();

        // Look for sample txs (they all start with 0) and gather their txs id (the next 8 bytes)
        // #[cfg(feature = "benchmark")]
        let tx_ids: Vec<_> = batch_tx
            .iter()
            .filter(|tx| tx.is_sample_tx())
            .map(|tx| tx.clone())
            .collect();

        // Serialize the batch.
        let batch = Batch{external_tx_nums: packaged_batch_external_txs, tx_list: batch_tx};      

        let message = WorkerMessage::Batch(batch);
        let serialized = bincode::serialize(&message).expect("Failed to serialize our own batch");

        // #[cfg(feature = "benchmark")]
        {
            // NOTE: This is one extra hash that is only needed to print the following log entries.
            let digest = Digest(
                Sha512::digest(&serialized).as_slice()[..32]
                    .try_into()
                    .unwrap(),
            );

            for id in tx_ids {
                // NOTE: This log entry is used to compute performance.
                info!(
                    "Batch {:?} contains sample tx {}",
                    digest,
                    id.get_sample_tx_counter(),
                    //u64::from_be_bytes(id)                    
                );
            }

            // NOTE: This log entry is used to compute performance.
            self.total_packaged_external_txs += packaged_batch_external_txs;
            info!("Batch {:?} contains {} B and {} external txs, {} total_packaged_external_txs", digest, packaged_batch_size, packaged_batch_external_txs, self.total_packaged_external_txs);
        }

        // Broadcast the batch through the network.
        let (names, addresses): (Vec<_>, _) = self.workers_addresses.iter().cloned().unzip();
        let bytes = Bytes::from(serialized.clone());

        let handlers = self.network.broadcast(addresses, bytes).await;

        // Send the batch through the deliver channel for further processing.
        self.tx_message
            .send(QuorumWaiterMessage {
                batch: serialized,
                handlers: names.into_iter().zip(handlers.into_iter()).collect(),
            })
            .await
            .expect("Failed to deliver batch");
    }
}
