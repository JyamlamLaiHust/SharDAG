// Copyright(C) Facebook, Inc. and its affiliates.
use crate::worker::WorkerMessage;
use bytes::Bytes;
use config::{Committee, WorkerId};
use crypto::{Digest, PublicKey};
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt as _;
use log::{info, debug, error};
use network::SimpleSender;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use store::{Store, StoreError};
use tokio::sync::mpsc::{channel, Sender};
use tokio::time::{sleep, Duration};
use tokio::sync::oneshot;


/// Resolution of the timer managing retrials of sync requests (in ms).
// 管理同步请求重试的间隔时间（单位：毫秒）
const TIMER_RESOLUTION: u64 = 1_000;
// 定义存储操作结果的返回类型
pub type StoreResult<T> = Result<T, StoreError>;

// 定义 MissingBatchFetcher 的支持命令
pub enum MissingBatchFetcherrCommand { 
  // digests, target 接受缺失批次的摘要列表、目标节点和回调通道
  FetchMissingBatches(Vec<Digest>, PublicKey, oneshot::Sender<StoreResult<bool>>), // called by TxConvertor
}

// 管理和获取缺失的批次
#[derive(Clone)]
pub struct MissingBatchFetcher {
    channel: Sender<MissingBatchFetcherrCommand>,
}


impl MissingBatchFetcher {
    /// Helper function. It waits for a batch to become available in the storage
    /// and then delivers its digest.
    // 辅助函数：等待某个批次在存储中可用，并返回其摘要
    async fn waiter(
      missing: Digest, // 缺失的批次摘要
      mut store: Store, // 存储对象，用于检查批次是否可用
      deliver: Digest, // 需要返回的摘要
    ) -> Result<Digest, StoreError> {
        // 使用 select 监听存储的通知，确认批次可用
        tokio::select! {
            result = store.notify_read(missing.to_vec()) => {
                result.map(|_| deliver) // 如果批次可用，返回对应的摘要
            }
        }
    }

  // 构造函数
  pub fn new(
    name: PublicKey,
    id: WorkerId,
    committee: Committee,
    store: Store,
    sync_retry_delay: u64,
    sync_retry_nodes: usize,
  ) -> Self {

    let mut network: SimpleSender = SimpleSender::new();
    let mut pending: HashMap<Digest, u128> = HashMap::new(); // all missing batches
    let mut waiting = FuturesUnordered::new(); // A set of futures which may complete in any order.

    let (tx, mut rx) = channel(100);
      // 启动异步任务，用于处理接收到的命令
      tokio::spawn(async move {
          while let Some(command) = rx.recv().await {
              match command {
                  MissingBatchFetcherrCommand::FetchMissingBatches(digests, target, sender) => {
                    info!("Fetch missing batches. digests: {:?}, target: {:?}", digests, target);

                    let now = SystemTime::now()
                      .duration_since(UNIX_EPOCH)
                      .expect("Failed to measure time")
                      .as_millis();

                    let mut missing = Vec::new();
                    for digest in digests { // create a waiter for each digest
                        missing.push(digest.clone());
                        // Add the digest to the waiter.
                        let deliver = digest.clone();
                        let fut = Self::waiter(digest.clone(), store.clone(), deliver);
                        waiting.push(fut);
                        pending.insert(digest, now);
                    }

                    // Send sync request to a single node (target). If this fails, we will send it
                    // to other nodes when a timer times out.
                    let address = match committee.worker(&target, &id) {
                        Ok(address) => address.worker_to_worker,
                        Err(e) => {
                            error!("The primary asked us to sync with an unknown node: {}", e);
                            continue;
                        }
                    };
                    let message = WorkerMessage::BatchRequest(missing, name);
                    let serialized = bincode::serialize(&message).expect("Failed to serialize our own message");
                    network.send(address, Bytes::from(serialized)).await;

                    // wait rep
                    let timer = sleep(Duration::from_millis(TIMER_RESOLUTION));
                    tokio::pin!(timer);
            
                    loop {
                        tokio::select! {
                            // Stream out the futures of the `FuturesUnordered` that completed.
                            Some(result) = waiting.next() => match result {
                                Ok(digest) => {
                                    // We got the batch, remove it from the pending list.
                                    info!("we get the missing batch: {:?}", digest);
                                    pending.remove(&digest);
                                    if pending.len() == 0 {
                                      info!("All missing batches has been fetched!");
                                      break;
                                    }
                                }
                                Err(e) => error!("{}", e)
                            },
            
                            // Triggers on timer's expiration.
                            () = &mut timer => {
                                // We optimistically sent sync requests to a single node. If this timer triggers,
                                // it means we were wrong to trust it. We are done waiting for a reply and we now
                                // broadcast the request to a bunch of other nodes (selected at random).
                                let now = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .expect("Failed to measure time")
                                    .as_millis();
            
                                let mut retry = Vec::new();
                                for (digest, timestamp) in &pending {
                                    if timestamp + (sync_retry_delay as u128) < now {
                                        debug!("Requesting sync for batch {} (retry)", digest);
                                        retry.push(digest.clone());
                                    }
                                }
                                if !retry.is_empty() {
                                    let addresses = committee
                                        .others_workers(&name, &id)
                                        .iter().map(|(_, address)| address.worker_to_worker)
                                        .collect();
                                    let message = WorkerMessage::BatchRequest(retry, name);
                                    let serialized = bincode::serialize(&message).expect("Failed to serialize our own message");
                                    network
                                        .lucky_broadcast(addresses, Bytes::from(serialized), sync_retry_nodes)
                                        .await;
                                }
                            },
                        }
                    }

                    let _ = sender.send(Ok(true));
                  }
              }
          }
      });
      Self { channel: tx }
  }

  // 请求获取缺失的批次
  pub async fn fetch_missing_batches(&mut self, digests: Vec<Digest>, target: PublicKey) -> StoreResult<bool> {
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self
        .channel
        .send(MissingBatchFetcherrCommand::FetchMissingBatches(digests, target, sender))
        .await
    {
        panic!("Failed to send FetchMissingBatches command to MissingBatchFetcher: {}", e);
    }
    receiver
        .await
        .expect("Failed to receive reply to FetchMissingBatches command from MissingBatchFetcher")
  }
}