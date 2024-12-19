use std::collections::HashMap;
use config::{ShardId, NodeId};
use network::{ReliableSender,CancelHandler};
use crypto::{PublicKey, SignatureService};
use std::net::SocketAddr;
use tokio::sync::mpsc::Receiver;
use log::{info, debug};
use crate::utils::shuffle_node_id_list;
use bytes::Bytes;
use crate::messages::Height;
use crate::worker::SendCSMessage;
use crate::messages::CSMsg;

// 用于处理跨分片消息，并将其发送到 broker 客户端
pub struct Send2Broker {
  shard_id: ShardId,
  node_id: NodeId,
  is_malicious: bool,
  _shard_num: usize,
  shard_size: usize,

  name: PublicKey,
  signature_service: SignatureService,

  client: SocketAddr,
  cs_sender_nums: usize,
  cs_rev_nums: usize, // number of cross-shard receivers 

  rx_process_txs: Receiver<SendCSMessage>,
  /// A network sender to broadcast the batches to the other workers.
  network: ReliableSender,

  /// Keeps the cancel handlers of the messages we sent.
  cancel_handlers: HashMap<Height, Vec<CancelHandler>>,

  tx1_id: u64, // the id of tx1 sent to broker client
}



impl Send2Broker {
  // 启动 Send2Broker 实例
  #[allow(clippy::too_many_arguments)]
  pub fn spawn(
    // shard config
    shard_id: ShardId,
    node_id: NodeId,
    is_malicious: bool,
    shard_num: usize,
    shard_size: usize,
    name: PublicKey,
    signature_service: SignatureService,
  
    cs_sender_nums: usize,
    cs_rev_nums: usize,
    client: SocketAddr, // addr of broker client

    rx_process_txs: Receiver<SendCSMessage>,
  ) {
      
      tokio::spawn(async move {
        Self {
          shard_id,
          node_id,
          is_malicious,
          _shard_num: shard_num,
          shard_size,
          name,
          signature_service,
          client,
          cs_sender_nums,
          cs_rev_nums,
          rx_process_txs,
          network: ReliableSender::new(),
          cancel_handlers: HashMap::with_capacity(2 * 50 as usize),// TODO 
          tx1_id: 0,
        }
        .run()
        .await;
    });

  }

  /// Main loop listening to the messages.
  // 监听并处理跨分片消息
  async fn run(&mut self) {

    info!(
      "Send2Broker is running!"
    );
    info!("client addr: {:?}", self.client);
    info!("nodeid: {:?}", self.node_id);
    info!(
      "cs_sender_nums: {}", self.cs_sender_nums
    );   
    info!(
      "cs_rev_nums: {}", self.cs_rev_nums
    );      


    
    while let Some(SendCSMessage{height, target_shard, tx }) = self.rx_process_txs.recv().await {
      debug!("receiving csmsg to shard {}: {:?}", target_shard, tx);

      // malicious cs node does not process csmsg
      // 如果节点是恶意节点，则不处理跨分片消息
      if self.is_malicious {
        continue;
      }   


      // generate CSMsg
      // 生成跨分片消息
      let mut csmsg = CSMsg::new(self.shard_id, target_shard,self.tx1_id, tx, &self.name, &mut self.signature_service).await;
      csmsg.set_sig(&mut self.signature_service).await;


      self.tx1_id += 1;

      // let msg_id = format!("[{}-{}]", csmsg.source_shard, csmsg.csmsg_sequence);
      // info!("send csmsg: {}, counter: {:?}", msg_id, csmsg.get_counter().await);
      

      // get the list of csmsg senders
      // 获取当前消息的发送节点列表
      let candi_node_id_list = shuffle_node_id_list(self.shard_size, &csmsg.inner_tx_hash);
      let sender_ids = &candi_node_id_list[0..self.cs_sender_nums];
      debug!("candi_node_id_list: {:?}", candi_node_id_list);
      debug!("sender_ids: {:?}", sender_ids);

      // 如果当前节点在发送节点列表中，则发送消息
      if sender_ids.contains(&(self.node_id as usize)) { // this node is a csmsg sender
        let bytes = bincode::serialize(&csmsg).expect("Failed to serialize our vote");
  
        // send tx1_msg to broker client
        debug!(
          "Send cross_shard msg {:?}, , counter: {:?}",
            csmsg.csmsg_sequence, csmsg.get_counter().await,
        );

        let handler = self.network.send(self.client, Bytes::from(bytes.clone())).await;
        self.cancel_handlers
          .entry(height)
          .or_insert_with(Vec::new)
          .push(handler);
      }
    }
  }
}

