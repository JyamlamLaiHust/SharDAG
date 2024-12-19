// Copyright(C) Facebook, Inc. and its affiliates.
use crate::messages::Certificate;
use crate::primary::PrimaryWorkerMessage;
use bytes::Bytes;
use config::Committee;
use crypto::PublicKey;
use network::SimpleSender;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use crate::primary::Height;
use log::debug;


/// Receives the highest round reached by consensus and update it for all tasks.
// 接收共识模块达到的最高轮次，并更新所有任务的状态
pub struct GarbageCollector {
    /// The current consensus round (used for cleanup).
    consensus_round: Arc<AtomicU64>,
    /// Receives the ordered certificates from consensus.
    rx_consensus: Receiver<Certificate>,
    /// The network addresses of our workers.
    addresses: Vec<SocketAddr>,
    /// A network sender to notify our workers of cleanup events.
    network: SimpleSender,
    /// block height
    height: Height,
}

// 创建并启动垃圾回收器任务
impl GarbageCollector {
    pub fn spawn(
        name: &PublicKey,
        committee: &Committee,
        consensus_round: Arc<AtomicU64>,
        rx_consensus: Receiver<Certificate>,
    ) {
        let addresses = committee
            .our_workers(name)
            .expect("Our public key or worker id is not in the committee")
            .iter()
            .map(|x| x.primary_to_worker)
            .collect();
        let genesis_height: u64 = 0;
        tokio::spawn(async move {
            Self {
                consensus_round,
                rx_consensus,
                addresses,
                network: SimpleSender::new(),
                height: genesis_height,
            }
            .run()
            .await;
        });
    }

    // 垃圾回收器的主运行逻辑
    async fn run(&mut self) {
        // 记录最后提交的轮次
        let mut last_committed_round = 0; 
        while let Some(certificate) = self.rx_consensus.recv().await {
            // TODO [issue #9]: Re-include batch digests that have not been sequenced into our next block.
            // 将未排序的批次摘要重新包含到下一个区块中
            let round = certificate.round(); // 获取证书的轮次
            if round > last_committed_round {
                last_committed_round = round; // 更新最后提交的轮次

                // Trigger cleanup on the primary.
                // 触发主节点的清理操作
                self.consensus_round.store(last_committed_round, Ordering::Relaxed);
            }
            // Trigger cleanup on the workers.
            // 触发工作节点的清理操作
            self.height += 1;     
            
            debug!(
              "Receiving committed certificate: {}, last_committed_round: {}, height: {}",
              certificate.header,
              last_committed_round,
              self.height
            );
            // 序列化清理消息并广播给工作节点
            let bytes = bincode::serialize(&PrimaryWorkerMessage::Cleanup(self.height, certificate.header.clone()))
                .expect("Failed to serialize our own message");
            self.network
                .broadcast(self.addresses.clone(), Bytes::from(bytes))
                .await;            
        }
    }
}
