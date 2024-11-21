use std::collections::HashMap;
use std::sync::Arc;
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


pub struct SendCSMsg {
  shard_id: ShardId,
  node_id: NodeId,
  is_malicious: bool,
  _shard_num: usize,
  shard_size: usize,

  name: PublicKey,
  signature_service: SignatureService,

  all_id_pubkey_map: Arc<HashMap<(ShardId, NodeId), (PublicKey, SocketAddr)>>,
  cs_sender_nums: usize,
  cs_rev_nums: usize, // number of cross-shard receivers 

  rx_process_txs: Receiver<SendCSMessage>,
  /// A network sender to broadcast the batches to the other workers.
  network: ReliableSender,

  /// Keeps the cancel handlers of the messages we sent.
  cancel_handlers: HashMap<Height, Vec<CancelHandler>>,

  cs_msg_id: Vec<u64>,
}



impl SendCSMsg {
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
  
    all_id_pubkey_map: Arc<HashMap<(ShardId, NodeId), (PublicKey, SocketAddr)>>,
    cs_sender_nums: usize,
    cs_rev_nums: usize,

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
          all_id_pubkey_map,
          cs_sender_nums,
          cs_rev_nums,
          rx_process_txs,
          network: ReliableSender::new(),
          cancel_handlers: HashMap::with_capacity(2 * 50 as usize),// TODO 
          cs_msg_id: vec![0; shard_num as usize],
        }
        .run()
        .await;
    });

  }

  /// Main loop listening to the messages.
  async fn run(&mut self) {

    info!(
      "CSMsgSender is running!"
    );
    info!(
      "cs_rev_nums: {}", self.cs_rev_nums
    );      

    
    while let Some(SendCSMessage{height, target_shard, tx }) = self.rx_process_txs.recv().await {
      debug!("receiving csmsg to shard {}: {:?}", target_shard, tx);

      // malicious cs node does not process csmsg
      if self.is_malicious {
        continue;
      }

      // get csmsg_seq
      let cs_msg_id = self.cs_msg_id.get_mut(target_shard).unwrap();
      let csmsg_seq = *cs_msg_id;
      *cs_msg_id += 1;

      // generate CSMsg
      let mut csmsg = CSMsg::new(self.shard_id, target_shard,csmsg_seq, tx, &self.name, &mut self.signature_service).await;
      csmsg.set_sig(&mut self.signature_service).await;
      

      // send this csmsg to target shard
      let candi_node_id_list = shuffle_node_id_list(self.shard_size, &csmsg.inner_tx_hash);
      let sender_ids = &candi_node_id_list[0..self.cs_sender_nums];
      let receiver_ids = &candi_node_id_list[0..self.cs_rev_nums];
      debug!("candi_node_id_list: {:?}", candi_node_id_list);
      debug!("sender_ids: {:?}", sender_ids);
      debug!("receiver_ids: {:?}", receiver_ids);

      if sender_ids.contains(&(self.node_id as usize)) { // this node is a sender
        let bytes = bincode::serialize(&csmsg).expect("Failed to serialize our vote");
  
        let mut addresses: Vec<SocketAddr> = Vec::new();
        for recv_id in receiver_ids {
          let addr = self.all_id_pubkey_map.get(&(target_shard, *recv_id as u32)).unwrap().1;
          addresses.push(addr);
        }
  
        debug!(
          "Send cross_shard msg {:?} to: [nodeid: {:?}]{:?}",
            csmsg.get_digest(), receiver_ids, addresses,
        );
        let handlers = self.network.broadcast(addresses, Bytes::from(bytes.clone())).await;
        self.cancel_handlers
          .entry(height)
          .or_insert_with(Vec::new)
          .extend(handlers);
      }
    }
  }
}