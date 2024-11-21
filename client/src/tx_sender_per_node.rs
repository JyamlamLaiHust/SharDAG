use bytes::{/*BytesMut, */Bytes};
use config::ShardId;
use futures::sink::SinkExt as _;
use log::{info, debug};
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use worker::{GeneralTransaction, Transaction};
use tokio::sync::mpsc::Receiver;


// Manage connections to blockchain nodes and send transactions
pub struct TxSenderPerNode {

  // send tx
  shardid: ShardId,
  nodeid: usize,

  transport: Framed<TcpStream, LengthDelimitedCodec>,

  rx_send_tx: Receiver<Transaction>,
}

impl TxSenderPerNode {
  pub async fn spawn(
    shardid: ShardId,
    nodeid: usize,
    transport: Framed<TcpStream, LengthDelimitedCodec>,
    rx_send_tx: Receiver<Transaction>,
  ) {

    let mut tx_sender = Self {
      shardid,
      nodeid,
      transport,
      rx_send_tx,
    };

    tokio::spawn(async move {
        // send tx to specific node 
        tx_sender.run().await;
    });
  }

  /// Main loop listening to the messages.
  async fn run(&mut self) {

    info!("TxSender for node ({}, {}) is running!", self.shardid, self.nodeid);
    while let Some(tx) = self.rx_send_tx.recv().await {
        let general_tx = GeneralTransaction::TransferTx(tx);
        let bytes = bincode::serialize(&general_tx).expect("Failed to serialize our transaction");
        debug!("send tx {:?} to node ({}, {})", general_tx.get_counter(), self.shardid, self.nodeid);

        // send tx
        if let Err(e) = self.transport.send(Bytes::from(bytes)).await {
          debug!("Failed to send transaction: {}", e);
          break;
        }
        debug!("send tx successfully");
    }
    info!("tx sender of node ({}, {}) exits!", self.shardid, self.nodeid);
  }
}

