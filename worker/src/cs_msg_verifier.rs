use std::{collections::HashMap, sync::Arc};
use config::{Stake, Committees, NodeId, ShardId};
use crypto::{assemble_intact_ts_partial, PublicKey, verify_ts_sig, Digest};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{self, Instant};
use crate::csmsg_store::{CSMsgStore, AppendedType};
use crate::utils::shuffle_node_id_list;
use crate::messages::{GeneralTransaction, CSMsg};
use log::{info, debug};
use num_enum::TryFromPrimitive;

pub const TIMER_RESOLUTION: u64 = 2500; // ms
pub const SAMPLE_CSMSG_DUR: usize = 100; // sample csmsg appending delay every 100 csmsgs
pub const OPTAPPEDNING: usize = 2; // 2 nodes in opt appending

// 定义追加消息的类型
#[derive(TryFromPrimitive, Debug)]
#[repr(usize)]
pub enum AppendType {
  DualMode, //双模式追加
  Serial, //串行追加
}

// 用于验证跨分片消息的有效性，并处理验证后的消息
pub struct CSMsgVerifier {
    rx_cross_shard_msg: Receiver<CSMsg>, // 接受跨分片消息的通道
    tx_batch_maker: Sender<GeneralTransaction>,
    all_committees: Committees,

    vote_threshold: Stake,
    shard_size: usize,

    nodeid: NodeId,
    is_malicious: bool,

    // all_pubkey_id_map: Arc<HashMap<PublicKey, (ShardId, NodeId)>>,
    
    csmsg_store: CSMsgStore,
    sampled_csmsg_num: usize,
}

impl CSMsgVerifier {
  pub fn spawn(
      rx_cross_shard_msg: Receiver<CSMsg>,
      tx_batch_maker: Sender<GeneralTransaction>,
      all_committees: Committees,
      vote_threshold: Stake,
      shard_size: usize,
      nodeid: NodeId,
      is_malicious: bool,
      _all_pubkey_id_map: Arc<HashMap<PublicKey, (ShardId, NodeId)>>,
      csmsg_store: CSMsgStore,
    ) {
      tokio::spawn(async move {
        Self {
          rx_cross_shard_msg,
          tx_batch_maker,
          all_committees,
          vote_threshold,// f+1
          shard_size,
          nodeid,
          is_malicious,
          csmsg_store,
          sampled_csmsg_num: 0,
        }
        .run()
        .await;
      });
    }

    /// Main loop listening to the messages.
    // 主循环：监听跨分片消息并进行验证
    async fn run(&mut self) {
      info!(
        "CrossShardMsgVer-DualMode is running!"
      );

      while let Some(cs_msg) = self.rx_cross_shard_msg.recv().await {
        debug!(
          "Receiving CSMsg: {:?}, csmsg_seq: {:?}",
          cs_msg, cs_msg.csmsg_sequence,
        );
        // malicious cs node does not process csmsg
        if self.is_malicious {
          continue;
        }

        if cs_msg.verify(&self.all_committees) {
          self.process_msg(cs_msg).await;
        } else {
          debug!(
            "threshold signature verification failed",
          );
        }
      }
    }

    // 处理验证通过的 CSMsg
    async fn process_msg(
      &mut self,
      csmsg: CSMsg,
    ) {
        // get the msgId of the csmsg
        let msg_id = format!("[{}-{}]", csmsg.source_shard, csmsg.csmsg_sequence);
        debug!("process csmsg: {}", msg_id);

        // try to add this csmsg sig to csmsg_store
        let (is_added, thres_sig_list) = self.csmsg_store.add_csmsg_sig(msg_id.clone(), csmsg.thres_sig).await.unwrap();
        debug!("res of add_csmsg_sig: {:?}, {:?}", is_added, thres_sig_list);
        if is_added {
          if thres_sig_list.len() != 0 { // reach vote_threshold, 
              // assemble intact ts partial and verify ts
              let inner_tx_hash = csmsg.inner_tx_hash;
              let intact_sig = assemble_intact_ts_partial(thres_sig_list, PublicKey::default(), &inner_tx_hash, self.vote_threshold, self.shard_size);

              if verify_ts_sig(PublicKey::default(), &inner_tx_hash, &intact_sig) {
                // pass verification
                let mut inner_tx = csmsg.tx;
                // update intact_sig of inner_tx
                inner_tx.set_thres_sig(intact_sig, csmsg.source_shard);

                // 追加消息到 DAG
                let msg_num = self.sampled_csmsg_num;
                self.sampled_csmsg_num += 1;
                let tx_batch_maker_pes = self.tx_batch_maker.clone();
                let csmsg_store = self.csmsg_store.clone();
                let nodeid = self.nodeid;
                let shard_size = self.shard_size;
                tokio::spawn(async move {
                  let begin = Instant::now();
                  append_msg(
                    tx_batch_maker_pes, csmsg_store, nodeid, shard_size,
                    msg_id.clone(), inner_tx_hash, inner_tx
                  ).await;
                  let append_dur = begin.elapsed().as_millis();
                  if msg_num % SAMPLE_CSMSG_DUR == 0 {
                    info!("{}, csmsg {} append delay: {:?} ms", msg_num, msg_id, append_dur);
                  }                  
                });
              } else {
                debug!("verify threshold signature failed");
              }
          }
        } else { // this csmsg has been validated or appended, just ignore it
          debug!("ignore this csmsg: {:?}", msg_id);
        }
    }
}

// 追加 CSMsg 消息
async fn append_msg(
  tx_batch_maker: Sender<GeneralTransaction>,
  mut csmsg_store: CSMsgStore,
  nodeid: u32, 
  shard_size: usize,
  msg_id: String,
  inner_tx_hash: Digest,
  inner_tx: GeneralTransaction,
) {
  
  // get packagers
  let candi_node_id_list = shuffle_node_id_list(shard_size, &inner_tx_hash);
  let opt_packagers = &candi_node_id_list[0..OPTAPPEDNING];
  debug!("candi_node_id_list: {:?}", candi_node_id_list);   
  debug!("opt_packagers: {:?}", opt_packagers);
  
  // two-mode
  if opt_packagers.contains(&(nodeid as usize)) { // opt appending
    // update csmsg status
    let updated = csmsg_store.update_appended(msg_id, AppendedType::Local).await.unwrap();
    if updated { // update csmsg status from validated to appended successfully, 
      debug!("opt csmsg packager, pack this csmsg!");
      // send GeneralTransaction to BatchMaker
      tx_batch_maker
        .send(inner_tx)
        .await
        .expect("Failed to send cs msg");
    } // check if this csmsg has been appended again. if appended, do not pack it locally. 
  } else { // pes appending, wait timeout
      let msg_id_clone = msg_id.clone();
      let res = time::timeout(time::Duration::from_millis(TIMER_RESOLUTION), async {
        let is_appended = csmsg_store.notify_appended(msg_id_clone.clone()).await.unwrap();
        is_appended 
      });
      match res.await {
          Err(_) => { // pes appending
            debug!("opt appending for csmsg: [{}] failed", msg_id_clone);
            let updated = csmsg_store.update_appended(msg_id_clone, AppendedType::Local).await.unwrap();
            if updated { // update csmsg status from validated to appended successfully, 
              debug!("pes appending!");
              tx_batch_maker
                .send(inner_tx)
                .await
                .expect("Failed to send cs msg");
            } // check if this csmsg has been appended again. if appended, do not pack it locally.
          },
          Ok(_) => { // this csmsg has been appended before timeout
            debug!(" csmsg: [{}] has been appended before timeout", msg_id_clone);
          }
      };
  }
}