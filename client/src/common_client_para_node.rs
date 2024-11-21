use anyhow::{Context, Result};
use config::ShardId;
use hex::FromHex;
use crate::convert_tx::ConvertTx;
use worker::{RawTxOld, RWSet, Account2ShardType, Account2Shard, Account2ShardGraph, Account2ShardHash, CoreTx, Frame, Transaction};
use futures::future::join_all;
use log::{info, warn, debug};
use rand::Rng;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::{interval, sleep, Duration, Instant};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use csv::DeserializeRecordsIter;
use std::fs::File;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::channel;
use crate::tx_sender_per_node::TxSenderPerNode;

// the channel capacity of tx_sender
pub const CHANNEL_CAPACITY_TX_SENDER: usize = 1000000;
type NodeId = usize;

pub struct CommonClientMultiTxSenderPerNode {
    // system settings
    shardnum: usize,
    shardsize: usize,

    // workload
    workload_file: String,
    acc_shard: Arc<dyn Account2Shard + Send>,
    _convert_tx: ConvertTx,

    // params of tx sending
    rate: u64,
    total_txs: u32, // total number of injected transactions
    send_tx_duration_ms: u32, // ms

    // about tx sending 
    nodes: Vec<SocketAddr>,
    next_node_id: Vec<usize>,

    tx_senders: HashMap<(ShardId, NodeId), Sender<Transaction>>,
}


impl CommonClientMultiTxSenderPerNode {
    pub async fn spawn(
      shardnum: usize,
      shardsize: usize,
      nodes: Vec<SocketAddr>,
      workload_file: String,
      acc2shard_file: String,
      acc_shard_type: Account2ShardType,
      rate: u64,
      total_txs: u32,
      send_tx_duration_ms: u32,  
    ) -> Result<()> {

      // crate acc_shard according to specified sharding policy
      let acc2shard: Arc<dyn Account2Shard + Send>;
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

      let mut client = CommonClientMultiTxSenderPerNode {
        shardnum,
        shardsize,
        workload_file,
        acc_shard: acc2shard,
        _convert_tx: ConvertTx::new(),
        rate,
        total_txs, 
        send_tx_duration_ms,
        nodes,
        next_node_id: vec![0; shardnum as usize],
        tx_senders: HashMap::default(),
      };

      info!("CommonClient (one TxSender for each node) is running!");
  
      // Wait for all nodes to be online and synchronized.
      client.wait().await;
  
      // Start the benchmark.
      client.send().await.context("Failed to submit transactions")
    }



    pub async fn send(&mut self) -> Result<()> {
        // Connect to the mempool.
        let mut all_worker_addrs: HashMap<ShardId, Vec<SocketAddr>> = HashMap::default();
        let mut addr_iter = self.nodes.iter();

        for shardid in 0..self.shardnum {
            all_worker_addrs.insert(shardid, Vec::default());
            for nodeid in 0..self.shardsize {
              let addr = addr_iter.next().unwrap();
              all_worker_addrs.get_mut(&shardid).unwrap().push(addr.clone());
              let stream = TcpStream::connect(addr)
                      .await
                      .context(format!("failed to connect to {}", addr))?;
              let transport = Framed::new(stream, LengthDelimitedCodec::new());
              // create a txsender for (shardid, nodeid)
              let (tx_txsender, rx_txsender) = channel(CHANNEL_CAPACITY_TX_SENDER);
              info!("node: ({}, {})", shardid, nodeid);
              self.tx_senders.insert((shardid, nodeid), tx_txsender);
              TxSenderPerNode::spawn(shardid, nodeid, transport, rx_txsender).await;
            }
        }
        info!("all_worker_addrs: {:?}", all_worker_addrs);


        const PRECISION: u64 = 20; // Sample precision.,
        const BURST_DURATION: u64 = 1000 / PRECISION; //sample interval：50ms

        // Submit all transactions.
        let burst = self.rate / PRECISION; // mark a tx every `burst` txs
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
            for x in 0..burst {// counter is the group seq (a group = `burst` txs). the sampled tx is determined by counter%burst
              if let Some(Ok(raw_tx_old)) = workload_iter.next() {

                if sent_txs >= self.total_txs {
                  break 'main; 
                }
                let mut tx_sample: u8 = 0;
                let mut tx_counter: u64 = counter;
                if x == counter % burst {// sample tx
                    // NOTE: This log entry is used to compute performance.
                    info!("Sending sample transaction {}", tx_counter);
                } else { // Standard tx
                    r += 1;
                    tx_sample = 1;
                    tx_counter = r;            
                };

                // info!("raw_tx: {:?}", raw_tx_old);
                // let begin = Instant::now();
                // generate tx
                // let core_tx = rawtx2tx(raw_tx_old, tx_sample, tx_counter);
                // let (tx, target_shard) = self.convert_tx.rawtx2tx(self.acc_shard.clone(), core_tx);
                let (tx, target_shard) = rawtx2simpletx(raw_tx_old, tx_sample, tx_counter, self.acc_shard.clone());
                // let total_dur = begin.elapsed().as_micros();
                // sample_convert_dur.push(total_dur);
                // info!("convert dur: {}", total_dur);
                // info!("tx: {:?}", tx);

              
                debug!("transaction: {:?}, target_shard: {}", tx, target_shard);

                // find the recv worker and send tx
                let index = self.next_node_id.get_mut(target_shard).unwrap();
                let nodeid = (*index) % self.shardsize as usize;
                debug!("send tx to :({}, {})", target_shard, nodeid);
                let tx_sender = self.tx_senders.get_mut(&(target_shard, nodeid)).unwrap();
                *index += 1;

                if let Err(_) = tx_sender.send(tx).await {
                  info!("tx_sender of shard: {} dropped!", target_shard);
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
        Ok(())
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
}



pub fn _rawtx2tx(
  raw_tx_old: RawTxOld,
  tx_sample: u8, 
  tx_counter: u64,
) -> CoreTx {
  // convert rawtx to general tx
  let sender = Vec::from_hex(&raw_tx_old.sender[2..]).unwrap();
  let receiver = Vec::from_hex(&raw_tx_old.receiver[2..]).unwrap();

  // conver this raw_tx_old to raw_tx
  let mut payload: Vec<RWSet> = Vec::new();
  payload.push(RWSet { addr: sender.clone(), value: raw_tx_old.amount * -1.0 });
  payload.push(RWSet { addr: receiver.clone(), value: raw_tx_old.amount});

  CoreTx::new(tx_sample, tx_counter, sender, receiver, raw_tx_old.amount, payload)
}


pub fn rawtx2simpletx(
  raw_tx_old: RawTxOld,
  tx_sample: u8, 
  tx_counter: u64,
  acc_shard: Arc<dyn Account2Shard + Send>,
) -> (Transaction, ShardId)  {
  // convert rawtx to simple tx
  let sender = Vec::from_hex(&raw_tx_old.sender[2..]).unwrap();
  let receiver = Vec::from_hex(&raw_tx_old.receiver[2..]).unwrap();

  // conver this raw_tx_old to raw_tx
  let mut frames: Vec<Frame> = Vec::new();
  let sender_rwset = RWSet { addr: sender.clone(), value: raw_tx_old.amount * -1.0 };
  let s_shardid  = acc_shard.get_shard(&sender) as usize;

  frames.push(Frame {shardid: s_shardid, rwset: vec![sender_rwset]});

  let receiver_rwset = RWSet { addr: receiver.clone(), value: raw_tx_old.amount};
  let r_shardid  = acc_shard.get_shard(&receiver) as usize;
  if r_shardid == s_shardid {
    frames.get_mut(0).unwrap().rwset.push(receiver_rwset);
  } else {
    frames.push(Frame {shardid: r_shardid, rwset: vec![receiver_rwset]});
  }

  let involved_shard_num = frames.len();
  let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros();

  let tx = Transaction::new(sender, receiver, raw_tx_old.amount, frames, 2, involved_shard_num,  tx_sample, tx_counter, timestamp, None, None);

  (tx,s_shardid)
}
