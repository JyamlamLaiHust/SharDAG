// Copyright(C) Facebook, Inc. and its affiliates.
use crate::worker::SerializedBatchDigestMessage;
use bytes::Bytes;
use network::SimpleSender;
use std::net::SocketAddr;
use tokio::sync::mpsc::Receiver;

// Send batches' digests to the primary.
// 该结构体用于将批次的摘要信息发送到主节点
pub struct PrimaryConnector {
    /// The primary network address.
    primary_address: SocketAddr,
    /// Input channel to receive the digests to send to the primary.
    rx_digest: Receiver<SerializedBatchDigestMessage>,
    /// A network sender to send the baches' digests to the primary.
    network: SimpleSender,
}

// 接收摘要并将其发送到主节点
impl PrimaryConnector {
    pub fn spawn(primary_address: SocketAddr, rx_digest: Receiver<SerializedBatchDigestMessage>) {
        tokio::spawn(async move {
            Self {
                primary_address,
                rx_digest,
                network: SimpleSender::new(),
            }
            .run()
            .await;
        });
    }

    // 处理接收到的摘要，并通过网络发送到主节点
    async fn run(&mut self) {
        while let Some(digest) = self.rx_digest.recv().await {
            // Send the digest through the network.
            self.network
                .send(self.primary_address, Bytes::from(digest))
                .await;
        }
    }
}
