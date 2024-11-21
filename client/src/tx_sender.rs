use bytes::{/*BytesMut, */Bytes};
use config::ShardId;
use futures::sink::SinkExt as _;
use log::{info, debug};
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use worker::{GeneralTransaction, Transaction};
use tokio::sync::mpsc::Receiver;


// Manage connections to blockchain nodes and send transactions
pub struct TxSender {
  _shardnum: usize,
  shardsize: usize,

  // send tx
  shardid: ShardId,
  next_node_id: usize,
  transports: Vec<Framed<TcpStream, LengthDelimitedCodec>>,

  rx_send_tx: Receiver<Transaction>,
}

impl TxSender {
  pub async fn spawn(
    shardnum: usize,
    shardsize: usize,
    shardid: ShardId,
    transports: Vec<Framed<TcpStream, LengthDelimitedCodec>>,
    rx_send_tx: Receiver<Transaction>,
  ) {

    let mut tx_sender = Self {
      _shardnum: shardnum,
      shardsize,
      shardid,
      next_node_id: 0,
      transports,
      rx_send_tx,
    };

    tokio::spawn(async move {
        // send tx to specific node 
        tx_sender.run().await;
    });
  }

  /// Main loop listening to the messages.
  async fn run(&mut self) {

    info!("TxSender for shard {} is running!", self.shardid);
    while let Some(tx) = self.rx_send_tx.recv().await {
        let general_tx = GeneralTransaction::TransferTx(tx);
        let bytes = bincode::serialize(&general_tx).expect("Failed to serialize our transaction");
        debug!("send tx {:?} to shard {}, node {}", general_tx.get_counter(), self.shardid, self.next_node_id);

        // find the recv worker and send tx
        let index = self.next_node_id;
        let transport = self.transports.get_mut(index % self.shardsize as usize).unwrap();
        self.next_node_id += 1;

        if let Err(e) = transport.send(Bytes::from(bytes)).await {
          debug!("Failed to send transaction: {}", e);
          break;
        }
        debug!("send tx successfully");
    }
    info!("tx sender of shard {} exits!", self.shardid);
  }
}

