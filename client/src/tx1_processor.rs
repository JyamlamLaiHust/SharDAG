use bytes::{/*BytesMut, */Bytes};
use anyhow::{Context, Result};
use config::ShardId;
use worker::Transaction;
use log::{info, debug};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio::time::{sleep, Duration};
use worker::GeneralTransaction;
use tokio::sync::mpsc::Receiver;
use futures::future::join_all;
use futures::sink::SinkExt as _;
use crate::broker::Broker;



pub struct Tx1Processor {
  shardnum: usize,
  shardsize: usize,

  nodes: Vec<SocketAddr>,
  next_node_id: Vec<usize>,
  transports: HashMap<ShardId, Vec<Framed<TcpStream, LengthDelimitedCodec>>>,

  rx_send_tx: Receiver<Transaction>,

  // broker
  broker: Broker,
}

impl Tx1Processor {
  pub async fn spawn(
    shardnum: usize,
    shardsize: usize,
    nodes: Vec<SocketAddr>,
    rx_send_tx: Receiver<Transaction>,
    broker: Broker,
  ) {

    let mut tx_sender = Self {
      shardnum,
      shardsize,
      nodes,
      next_node_id: vec![0; shardnum as usize],
      transports: HashMap::default(),
      rx_send_tx,
      broker,
    };

    // Wait for all nodes to be online and synchronized.
    tx_sender.wait().await;
    // connect to nodes
    let _ = tx_sender.conn().await; 

    tokio::spawn(async move {
        // send tx to specific node 
        tx_sender.run().await;
    });
  }

  /// Main loop listening to the messages.
  async fn run(&mut self) {

    info!("Tx1Processor is running!");
    
    while let Some(tx1) = self.rx_send_tx.recv().await {
        debug!("process tx1 {:?}", tx1.counter);

        // generate tx2
        let (tx2, target_shard) = self.broker.process_tx1(tx1).await.unwrap();
        debug!("tx2: {:?}, target_shard: {}", tx2, target_shard);
        self.send_tx(tx2, target_shard).await;
        debug!("send tx2 successfully");
    }

    info!("Tx2Processor exits!");
  }

  pub async fn send_tx(&mut self, tx: Transaction, target_shard: ShardId) -> bool {
    let general_tx = GeneralTransaction::TransferTx(tx);
    let bytes = bincode::serialize(&general_tx).expect("Failed to serialize our transaction");

    // find the recv worker
    let index = self.next_node_id.get_mut(target_shard).unwrap();
    let transport = self.transports.get_mut(&target_shard).unwrap().get_mut((*index) % self.shardsize as usize).unwrap();
    *index += 1;

    if let Err(e) = transport.send(Bytes::from(bytes)).await {
        debug!("Failed to send tx2: {}", e);
        return false;
    }
    true
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

  pub async fn wait(&self) {
    // Wait for all nodes to be online.
    info!("Waiting for all nodes to be online...");
    info!("wait nodes: {:?}", self.nodes);
    
    join_all(self.nodes.iter().cloned().map(|address| {
        tokio::spawn(async move {
            while TcpStream::connect(address).await.is_err() {
                sleep(Duration::from_millis(10)).await;
            }
        })
    }))
    .await;
    info!("All nodes are online now");
  }
}

