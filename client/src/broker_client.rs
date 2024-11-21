use async_trait::async_trait;
use bytes::{/*BytesMut, */Bytes};
use anyhow::{Context, Result};
use config::{ShardId, Committees};
use worker::{RawTxOld, Transaction, Account2ShardType, CSMsg, CSMsgStore};
use log::{info, warn, error, debug};
use rand::Rng;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio::time::{sleep, interval, Duration, Instant};
use worker::GeneralTransaction;
use csv::DeserializeRecordsIter;
use std::fs::File;
use tokio::sync::mpsc::{channel, Sender};
use futures::future::join_all;
use futures::sink::SinkExt as _;
use network::{MessageHandler, Receiver, Writer};
use std::error::Error;
use crate::broker::Broker;
use crate::common_client::rawtx2tx;
use crate::tx1_processor::Tx1Processor;
use crate::tx1_verifier::Tx1Verifier;

/// The default channel capacity for each channel of the client.
pub const CHANNEL_CAPACITY: usize = 1_000;

pub struct BrokerClient {
    // system settings
    shardnum: usize,
    shardsize: usize,

    // workload
    workload_file: String,
    // acc_shard: Arc<dyn Account2Shard + Send + Sync>,
    // convert_tx: ConvertTx,

    // params of tx sending
    rate: u64,
    total_txs: u32, // total number of injected transactions
    send_tx_duration_ms: u32, // ms

    // about tx sending 
    nodes: Vec<SocketAddr>,
    next_node_id: Vec<usize>,
    transports: HashMap<ShardId, Vec<Framed<TcpStream, LengthDelimitedCodec>>>,

    // broker
    broker: Broker,
}


impl BrokerClient {
    pub async fn spawn(
      shardnum: usize,
      shardsize: usize,
      nodes: Vec<SocketAddr>,
      workload_file: String,
      acc2shard_file: String,
      brokers_file: String,
      acc_shard_type: Account2ShardType,
      rate: u64,
      total_txs: u32,
      send_tx_duration_ms: u32,
      mut client_addr: SocketAddr,
      all_committees: Committees,
      epoch: usize,
    ) -> Result<()> {

      // crate broker module
      let broker = Broker::new(
        acc_shard_type,
        shardnum,
        acc2shard_file,
        brokers_file,
        epoch,
      );

      let mut client = BrokerClient {
        shardnum,
        shardsize,

        workload_file,
        rate,
        total_txs, 
        send_tx_duration_ms,

        nodes: nodes.clone(),
        next_node_id: vec![0; shardnum as usize],
        transports: HashMap::default(),
        
        broker: broker.clone(),
      };

      info!("BrokerClient is running!");

      // Receive incoming messages from workers.
      let (tx_cross_shard_msg, rx_cross_shard_msg) = channel(CHANNEL_CAPACITY);
      let (tx_process_tx1, rx_process_tx1) = channel(CHANNEL_CAPACITY);
      client_addr.set_ip("0.0.0.0".parse().unwrap());
      Receiver::spawn(
          client_addr,
          /* handler */
          CrossShardReceiverHandler { tx_cross_shard_msg },
      );

      let csmsg_store = CSMsgStore::new(all_committees.validity_threshold());

      Tx1Verifier::spawn(
        rx_cross_shard_msg,
        tx_process_tx1,
        all_committees.validity_threshold(),
        all_committees.shard_size(),
        all_committees,
        csmsg_store,
      );

      // create Tx1Processor
      Tx1Processor::spawn(
        shardnum,
        shardsize,
        nodes,
        rx_process_tx1,
        broker,
      ).await;


      // Wait for all nodes to be online and synchronized.
      client.wait().await;

      // connect to nodes
      let _ = client.conn().await;
  
      // read raw_tx from input_file and send tx
      client.send().await.context("Failed to submit transactions")
    }



    pub async fn send(&mut self) -> Result<()> {

        const PRECISION: u64 = 20; // Sample precision.,
        const BURST_DURATION: u64 = 1000 / PRECISION;

        // Submit all transactions.
        let burst = self.rate / PRECISION; 
        info!("sample interval: one per {} txs", burst);
        let mut counter = 0;
        let mut r = rand::thread_rng().gen();
        let interval = interval(Duration::from_millis(BURST_DURATION)); // 50ms
        tokio::pin!(interval);

        // open workload file
        let mut reader = csv::Reader::from_path(self.workload_file.clone()).unwrap();
        let mut workload_iter: DeserializeRecordsIter<File, RawTxOld> = reader.deserialize().into_iter();

        // NOTE: This log entry is used to compute performance.
        info!("Start sending transactions");
        let mut sent_txs = 0; 
        let begin_sending_txs = Instant::now();

        'main: loop {
            if begin_sending_txs.elapsed().as_millis() > self.send_tx_duration_ms as u128 {
              break 'main;
            }

            interval.as_mut().tick().await;
            let now = Instant::now();
            for x in 0..burst {
              if let Some(Ok(raw_tx_old)) = workload_iter.next() {

                if sent_txs >= self.total_txs {
                  break 'main; 
                }

                let mut tx_sample: u8 = 0;
                let mut tx_counter: u64 = counter;
                if x == counter % burst {// sample tx
                    info!("Sending sample transaction {}", tx_counter);
                } else { // Standard tx
                    r += 1;
                    tx_sample = 1;
                    tx_counter = r;
                };

                debug!("raw tx: {:?}", raw_tx_old);
                let core_tx = rawtx2tx(raw_tx_old, tx_sample, tx_counter);
                debug!("core tx: {:?}", core_tx);
                // broker processes the rawtx and convert it into tx
                let (tx, target_shard) = self.broker.convert_tx(core_tx).await.unwrap();
                debug!("transaction: {:?}, target_shard: {}", tx, target_shard);

                if !self.send_tx(tx, target_shard).await {
                  break 'main;
                }

                sent_txs += 1;

              }else{
                break 'main;
              }
            }// end of for
            if now.elapsed().as_millis() > BURST_DURATION as u128 {
                // NOTE: This log entry is used to compute performance.
                warn!("Transaction rate too high for this client");
            }
            counter += 1;
        }// main loop
        info!("Sending tx is finished! Send total {} txs!", sent_txs);
        
        loop {} // waiting processing tx1
        // Ok(())
    }


  pub async fn send_tx(&mut self, tx: Transaction, target_shard: ShardId) -> bool {
    let general_tx = GeneralTransaction::TransferTx(tx);
    let bytes = bincode::serialize(&general_tx).expect("Failed to serialize our transaction");

    // find the recv worker
    let index = self.next_node_id.get_mut(target_shard).unwrap();
    let transport = self.transports.get_mut(&target_shard).unwrap().get_mut((*index) % self.shardsize as usize).unwrap();
    *index += 1;

    if let Err(e) = transport.send(Bytes::from(bytes)).await {
        warn!("Failed to send transaction: {}", e);
        return false;
    }
    true
  }



  pub async fn wait(&self) {
      // Wait for all nodes to be online.
      info!("Waiting for all nodes to be online...");
      join_all(self.nodes.iter().cloned().map(|address| {
          tokio::spawn(async move {
              while TcpStream::connect(address).await.is_err() {
                  sleep(Duration::from_millis(10)).await;
              }
          })
      }))
      .await;
    info!("All nodes are online now");
    sleep(Duration::from_secs(5)).await;
  }


  pub async fn conn(&mut self) -> Result<()>{
    info!("connect to all nodes...");
    let mut all_worker_addrs: HashMap<ShardId, Vec<SocketAddr>> = HashMap::default();
    let mut addr_iter = self.nodes.iter();

    for shardid in 0..self.shardnum {
        all_worker_addrs.insert(shardid, Vec::default());
        self.transports.insert(shardid, Vec::default());
        for _nodeid in 1..self.shardsize+1 {
          let addr = addr_iter.next().unwrap();
          all_worker_addrs.get_mut(&shardid).unwrap().push(addr.clone());
          let stream = TcpStream::connect(addr)
                  .await
                  .context(format!("failed to connect to {}", addr))?;
          let transport = Framed::new(stream, LengthDelimitedCodec::new());
          self.transports.get_mut(&shardid).unwrap().push(transport);
        }
    }
    info!("all_worker_addrs: {:?}", all_worker_addrs);
    Ok(())
  }

}



/// Defines how the network receiver handles incoming transactions.
#[derive(Clone)]
struct CrossShardReceiverHandler {
    tx_cross_shard_msg: Sender<CSMsg>,
}

#[async_trait]
impl MessageHandler for CrossShardReceiverHandler {
    async fn dispatch(
      &self,
      _writer: &mut Writer, 
      serialized: Bytes,
    ) -> Result<(), Box<dyn Error>> {

        // Deserialize the message and send it to the batch maker.
        match bincode::deserialize(&serialized) {
            Err(e) => error!("Failed to deserialize cross shard msg: {}", e),
            Ok(message) => self
                .tx_cross_shard_msg
                .send(message)
                .await
                .expect("Failed to send cross shard msg"),
        }

        // Give the change to schedule other tasks.
        tokio::task::yield_now().await;
        Ok(())
    }
}