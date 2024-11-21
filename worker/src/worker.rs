use crate::cs_msg_verifier_serial::CSMsgVerifierSerial;
use crate::csmsg_store::CSMsgStore;
use crate::executor_b::BExecutor;
use crate::executor_m::MExecutor;
use crate::cs_msg_sender_b::Send2Broker;
use crate::batch_fetcher::MissingBatchFetcher;
use crate::{ExecutorType, Account2Shard, StateStore, StateTransition, AppendType};
use crate::batch_maker::{Batch, BatchMaker};
use crate::cs_msg_verifier::CSMsgVerifier;
use crate::helper::Helper;
use crate::messages::{Height, GeneralTransaction, CSMsg};
use crate::primary_connector::PrimaryConnector;
use crate::cs_msg_sender::SendCSMsg;
use crate::processor::{Processor, SerializedBatchMessage};
use crate::quorum_waiter::QuorumWaiter;
use crate::synchronizer::Synchronizer;
use crate::executor_s::SExecutor;
use crate::tx_convertor::TxConvertor;
use async_trait::async_trait;
use bytes::Bytes;
use config::{Committee, Parameters, WorkerId, ShardId, Committees};
use crypto::{Digest, PublicKey, SignatureService, SecretKey};
use futures::sink::SinkExt as _;
use log::{error, info, warn};
use network::{MessageHandler, Receiver, Writer};
use primary::{PrimaryWorkerMessage, Header};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use store::Store;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::mpsc::Receiver as ChannelReceiver;
use config::NodeId;


// #[cfg(test)]
// #[path = "tests/worker_tests.rs"]
// pub mod worker_tests;


/// The default channel capacity for each channel of the worker.
pub const CHANNEL_CAPACITY: usize = 1_000;

/// Indicates a serialized `WorkerPrimaryMessage` message.
pub type SerializedBatchDigestMessage = Vec<u8>;


/// The message exchanged between workers.
#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerMessage {
    Batch(Batch),
    BatchRequest(Vec<Digest>, /* origin */ PublicKey),
}


#[derive(Debug)]
pub struct ConversionMessage {
  pub height: Height,
  pub header: Header,
}

// #[derive(Debug)]
// pub struct FetchBatchMessage {
//   pub digests: Vec<Digest>, 
//   pub target: PublicKey,
// }


#[derive(Debug)]
pub struct SynchronizationMessage {
  pub height: Height,
  pub header: Header,
  pub batch_list: Vec<Batch>,  
}

#[derive(Debug)]
pub struct SendCSMessage {
  pub height: Height,
  pub target_shard: ShardId,
  pub tx: GeneralTransaction,
}


pub struct Worker {
    /// The committee information.
    committee: Committee,
    /// The shards information
    all_committees: Committees,    
    /// The configuration parameters.
    parameters: Parameters,

    /// info of this node
    name: PublicKey,
    id: WorkerId,
    shardid: ShardId,
    nodeid: NodeId,
    is_malicious: bool,
    
    /// The persistent storage.
    store: Store,
    csmsg_store: CSMsgStore,


    all_id_pubkey_map: Arc<HashMap<(ShardId, NodeId), (PublicKey, SocketAddr)>>,
    all_pubkey_id_map: Arc<HashMap<PublicKey, (ShardId, NodeId)>>,

    executor_type: ExecutorType,    
}

impl Worker {
    pub fn spawn(
        executor_type: ExecutorType, 
        append_type: AppendType, 
        name: PublicKey,
        secret: SecretKey,
        id: WorkerId,
        _cs_faults: usize,
        is_malicious: bool,
        committee: Committee,
        parameters: Parameters,
        store: Store,
        shardid: ShardId,
        all_committees: Committees,
        primary_store: Box<dyn StateStore + Send>, 
        account_shard: Box<dyn Account2Shard + Send>,
    ) {

        let mut all_id_pubkey_map: HashMap<(ShardId, NodeId), (PublicKey, SocketAddr)> = HashMap::new();
        let mut all_pubkey_id_map: HashMap<PublicKey, (ShardId, NodeId)> = HashMap::new();
        let mut nodeid = 0;
        // load the info of all shards 
        for (shardid_temp, committee) in all_committees.shards.iter() {
          let mut all_addrs: Vec<(PublicKey, SocketAddr)> = committee
          .all_worker_nodes(&id)
          .iter()
          .map(|(name, addresses)| (*name, addresses.cross_shard_worker))
          .collect();
          // sort all nodes 
          all_addrs.sort_by(|a, b| a.1.cmp(&b.1));

          for (node_id, value) in all_addrs.iter().enumerate(){
             all_id_pubkey_map.insert((*shardid_temp, node_id as u32), value.clone());
              all_pubkey_id_map.insert(value.0.clone(), (*shardid_temp, node_id as u32));

              if (*value).0 == name {
                  nodeid = node_id as u32;
              }
            }        
        }

        info!("[Shard config] Shard num: {}, Shard size: {}", all_committees.shard_num(), committee.size());
        info!(
          "[Node info] Node ({}, {}), Publickey: {:?}",
          shardid, nodeid, name, 
        );

        // info!("all_id_pubkey_map:");
        // for (key, value) in &all_id_pubkey_map{
        //   info!(
        //     "{:?} {:?}",
        //     key, value
        //   );
        // }
        // info!("all_pubkey_id_map:");
        // for (key, value) in &all_pubkey_id_map{
        //   info!(
        //     "{:?} {:?}",
        //     key, value
        //   );
        // }

        // Make the csmsg_status store.
        let csmsg_store = CSMsgStore::new(committee.validity_threshold());

        // Define a worker instance.
        let worker = Self {
          committee,
          all_committees,
          parameters,
          name,
          id,
          shardid,
          nodeid,
          is_malicious,
          store,
          csmsg_store,
          all_id_pubkey_map: Arc::new(all_id_pubkey_map),
          all_pubkey_id_map: Arc::new(all_pubkey_id_map),
          executor_type,
        };

        // Spawn all worker tasks.
        let (tx_primary, rx_primary) = channel(CHANNEL_CAPACITY);
        let(tx_process, rx_process) = channel(CHANNEL_CAPACITY);

        worker.handle_primary_messages(tx_process);
        worker.handle_clients_transactions(tx_primary.clone(), _cs_faults, append_type);
        worker.handle_workers_messages(tx_primary);
        worker.handle_tx_processing(rx_process, secret, primary_store, account_shard);        

        // The `PrimaryConnector` allows the worker to send messages to its primary.
        PrimaryConnector::spawn(
            worker
                .committee
                .primary(&worker.name)
                .expect("Our public key is not in the committee")
                .worker_to_primary,
            rx_primary,
        );

        // NOTE: This log entry is used to compute performance.
        info!(
            "Worker ({}, {}) successfully booted on {}!",
            worker.shardid,
            worker.nodeid,
            worker
                .committee
                .worker(&worker.name, &worker.id)
                .expect("Our public key or worker id is not in the committee")
                .transactions
                .ip(),            
        );
    }

    fn handle_tx_processing(
      &self, 
      rx_process: ChannelReceiver<ConversionMessage>,
      secret: SecretKey,
      primary_store: Box<dyn StateStore + Send>,
      account_shard: Box<dyn Account2Shard + Send>,
    ) {
      let signature_service = SignatureService::new(secret);
      let(tx_csmsg, rx_csmsg) = channel(CHANNEL_CAPACITY);
      let(tx_execution, rx_execution) = channel(CHANNEL_CAPACITY);


      let fetch_batch = MissingBatchFetcher::new(
        self.name, 
        self.id, 
        self.committee.clone(), 
        self.store.clone(), 
        self.parameters.sync_retry_delay,
        self.parameters.sync_retry_nodes,
      );

      TxConvertor::spawn(
        self.store.clone(),
        rx_process,
        tx_execution,
        fetch_batch,
      );   
      
      // create executor
      let state_transition = StateTransition::new(primary_store);
      match self.executor_type {
        ExecutorType::SharDAG => {
          SExecutor::spawn(
            self.shardid,
            rx_execution,
            tx_csmsg,
            state_transition,
            account_shard,
            self.csmsg_store.clone(),
          );

          SendCSMsg::spawn(
            self.shardid,
            self.nodeid,
            self.is_malicious,
            self.all_committees.shard_num(),
            self.committee.size(),
            self.name,
            signature_service,      
            self.all_id_pubkey_map.clone(),
            self.committee.quorum_threshold() as usize,
            self.committee.validity_threshold() as usize,
            rx_csmsg,         
          );
        },
        ExecutorType::Monoxide => {
          MExecutor::spawn(
            self.shardid,
            rx_execution,
            tx_csmsg,
            state_transition,
            account_shard,
            self.csmsg_store.clone(),
          );
          SendCSMsg::spawn(
            self.shardid,
            self.nodeid,
            self.is_malicious,
            self.all_committees.shard_num(),
            self.committee.size(),
            self.name,
            signature_service,      
            self.all_id_pubkey_map.clone(),
            self.committee.quorum_threshold() as usize,
            self.committee.validity_threshold() as usize,
            rx_csmsg,         
          );            
        },
        ExecutorType::BrokerChain => {
          BExecutor::spawn(
            self.shardid,
            rx_execution,
            tx_csmsg,
            state_transition,
            account_shard,
            self.csmsg_store.clone(),
          );

          let client_addr = self.all_committees.client;
          Send2Broker::spawn(
            self.shardid,
            self.nodeid,
            self.is_malicious,
            self.all_committees.shard_num(),
            self.committee.size(),
            self.name,
            signature_service,      
            self.committee.quorum_threshold() as usize,
            self.committee.validity_threshold() as usize,
            client_addr,
            rx_csmsg,         
          );     
        }            
      }
    }

    /// Spawn all tasks responsible to handle messages from our primary.
    fn handle_primary_messages(&self, tx_process: Sender<ConversionMessage>) {
        let (tx_synchronizer, rx_synchronizer) = channel(CHANNEL_CAPACITY);

        // Receive incoming messages from our primary.
        let mut address = self
            .committee
            .worker(&self.name, &self.id)
            .expect("Our public key or worker id is not in the committee")
            .primary_to_worker;
        address.set_ip("0.0.0.0".parse().unwrap());
        Receiver::spawn(
            address,
            /* handler */
            PrimaryReceiverHandler { tx_synchronizer },
        );

        // The `Synchronizer` is responsible to keep the worker in sync with the others. It handles the commands
        // it receives from the primary (which are mainly notifications that we are out of sync).
        Synchronizer::spawn(
            self.name,
            self.id,
            self.committee.clone(),
            self.store.clone(),
            self.parameters.gc_depth,
            self.parameters.sync_retry_delay,
            self.parameters.sync_retry_nodes,
            /* rx_message */ rx_synchronizer,
            tx_process,
        );

        info!(
            "[({}, {})] Worker {} listening to primary messages on {}",
            self.shardid, self.nodeid, self.id, address
        );
    }

    /// Spawn all tasks responsible to handle clients transactions.
    fn handle_clients_transactions(&self, 
      tx_primary: Sender<SerializedBatchDigestMessage>,
      _cs_faults: usize,
      _append_type: AppendType,
    ) {
        let (tx_batch_maker, rx_batch_maker) = channel(CHANNEL_CAPACITY * 2);
        let (tx_quorum_waiter, rx_quorum_waiter) = channel(CHANNEL_CAPACITY);
        let (tx_processor, rx_processor) = channel(CHANNEL_CAPACITY);

        let (tx_cross_shard_msg, rx_cross_shard_msg) = channel(CHANNEL_CAPACITY);
        
        let tx_batch_maker_2 = tx_batch_maker.clone();
        // We first receive clients' transactions from the network.
        let mut address_tx = self
            .committee
            .worker(&self.name, &self.id)
            .expect("Our public key or worker id is not in the committee")
            .transactions;
        address_tx.set_ip("0.0.0.0".parse().unwrap());
        Receiver::spawn(
            address_tx,
            /* handler */ TxReceiverHandler { tx_batch_maker},
        );

        // Receive incoming messages from other shards' workers.
        let mut address_cross_shard = self
            .committee
            .worker(&self.name, &self.id)
            .expect("Our public key or worker id is not in the committee")
            .cross_shard_worker;
        address_cross_shard.set_ip("0.0.0.0".parse().unwrap());
        Receiver::spawn(
            address_cross_shard,
            /* handler */
            CrossShardReceiverHandler { tx_cross_shard_msg },
        );

        // create CSMsgVerifier
        match _append_type { // test appending delay
            AppendType::DualMode => {
              CSMsgVerifier::spawn(
                rx_cross_shard_msg,
                tx_batch_maker_2,
                self.all_committees.clone(),
                self.committee.validity_threshold(),
                self.committee.size(),
                self.nodeid,
                self.is_malicious,
                self.all_pubkey_id_map.clone(),
                self.csmsg_store.clone(),
              );
            },
            AppendType::Serial => {
              CSMsgVerifierSerial::spawn(
                rx_cross_shard_msg,
                tx_batch_maker_2,
                self.all_committees.clone(),
                self.committee.validity_threshold(),
                self.committee.size(),
                self.nodeid,
                self.is_malicious,
                self.all_pubkey_id_map.clone(),
                self.csmsg_store.clone(),
              );
            }
          }
        // if cs_faults == 0 {
        //   CSMsgVerifierSerial::spawn(
        //     rx_cross_shard_msg,
        //     tx_batch_maker_2,
        //     self.all_committees.clone(),
        //     self.committee.validity_threshold(),
        //     self.committee.size(),
        //     self.nodeid,
        //     self.is_malicious,
        //     self.all_pubkey_id_map.clone(),
        //     self.csmsg_store.clone(),
        //   );
        // } else {
        //   match self.executor_type {
        //     ExecutorType::SharDAG => {
        //       CSMsgVerifier::spawn(
        //         rx_cross_shard_msg,
        //         tx_batch_maker_2,
        //         self.all_committees.clone(),
        //         self.committee.validity_threshold(),
        //         self.committee.size(),
        //         self.nodeid,
        //         self.is_malicious,
        //         self.all_pubkey_id_map.clone(),
        //         self.csmsg_store.clone(),
        //       );
        //     },
        //     _ => {
        //       CSMsgVerifierSerial::spawn(
        //         rx_cross_shard_msg,
        //         tx_batch_maker_2,
        //         self.all_committees.clone(),
        //         self.committee.validity_threshold(),
        //         self.committee.size(),
        //         self.nodeid,
        //         self.is_malicious,
        //         self.all_pubkey_id_map.clone(),
        //         self.csmsg_store.clone(),
        //       );
        //     }            
        //   }
        // }

          
        // The transactions are sent to the `BatchMaker` that assembles them into batches. It then broadcasts
        // (in a reliable manner) the batches to all other workers that share the same `id` as us. Finally, it
        // gathers the 'cancel handlers' of the messages and send them to the `QuorumWaiter`.
        BatchMaker::spawn(
            self.parameters.batch_size,
            self.parameters.max_batch_delay,
            /* rx_transaction */ rx_batch_maker,
            /* tx_message */ tx_quorum_waiter,
            /* workers_addresses */
            self.committee
                .others_workers(&self.name, &self.id)
                .iter()
                .map(|(name, addresses)| (*name, addresses.worker_to_worker))
                .collect(),
        );


        // The `QuorumWaiter` waits for 2f authorities to acknowledge reception of the batch. It then forwards
        // the batch to the `Processor`.
        QuorumWaiter::spawn(
            self.committee.clone(),
            /* stake */ self.committee.stake(&self.name),
            /* rx_message */ rx_quorum_waiter,
            /* tx_batch */ tx_processor,
        );

        // The `Processor` hashes and stores the batch. It then forwards the batch's digest to the `PrimaryConnector`
        // that will send it to our primary machine.
        Processor::spawn(
            self.id,
            self.nodeid,
            self.is_malicious,
            self.committee.size(),
            self.store.clone(),
            /* rx_batch */ rx_processor,
            /* tx_digest */ tx_primary,
            /* own_batch */ true,
            self.csmsg_store.clone(), // unused
            self.committee.validity_threshold(),
        );

        info!(
            "[({}, {})] Worker {} listening to client transactions on {}",
            self.shardid, self.nodeid, self.id, address_tx
        );
        info!(
          "[({}, {})] Worker {} listening to cross shard messages on {}",
          self.shardid, self.nodeid, self.id, address_cross_shard
      );
  }

    /// Spawn all tasks responsible to handle messages from other workers.
    fn handle_workers_messages(&self, tx_primary: Sender<SerializedBatchDigestMessage>) {
        let (tx_helper, rx_helper) = channel(CHANNEL_CAPACITY);
        let (tx_processor, rx_processor) = channel(CHANNEL_CAPACITY);

        // Receive incoming messages from other workers.
        let mut address = self
            .committee
            .worker(&self.name, &self.id)
            .expect("Our public key or worker id is not in the committee")
            .worker_to_worker;
        address.set_ip("0.0.0.0".parse().unwrap());
        Receiver::spawn(
            address,
            /* handler */
            WorkerReceiverHandler {
                tx_helper,
                tx_processor,
            },
        );

        // The `Helper` is dedicated to reply to batch requests from other workers.
        Helper::spawn(
            self.id,
            self.committee.clone(),
            self.store.clone(),
            /* rx_request */ rx_helper,
        );

        // This `Processor` hashes and stores the batches we receive from the other workers. It then forwards the
        // batch's digest to the `PrimaryConnector` that will send it to our primary.
        Processor::spawn(
            self.id,
            self.nodeid,
            self.is_malicious,
            self.committee.size(),
            self.store.clone(),
            /* rx_batch */ rx_processor,
            /* tx_digest */ tx_primary,
            /* own_batch */ false,
            self.csmsg_store.clone(),
            self.committee.validity_threshold(),
        );

        info!(
            "[({}, {})] Worker {} listening to worker messages on {}",
            self.shardid, self.nodeid, self.id, address
        );
    }

}

/// Defines how the network receiver handles incoming transactions.
#[derive(Clone)]
struct TxReceiverHandler {
    tx_batch_maker: Sender<GeneralTransaction>,
}

#[async_trait]
impl MessageHandler for TxReceiverHandler {
    async fn dispatch(
      &self,
      _writer: &mut Writer, 
      serialized: Bytes,
    ) -> Result<(), Box<dyn Error>> {

        // Deserialize the message and send it to the batch maker.
        match bincode::deserialize(&serialized) {
            Err(e) => error!("Failed to deserialize transfer tx: {}", e),
            Ok(message) => self
                .tx_batch_maker
                .send(message)
                .await
                .expect("Failed to send transaction"),
        }

        // Give the change to schedule other tasks.
        tokio::task::yield_now().await;
        Ok(())
    }
}

/// Defines how the network receiver handles incoming workers messages.
#[derive(Clone)]
struct WorkerReceiverHandler {
    tx_helper: Sender<(Vec<Digest>, PublicKey)>,
    tx_processor: Sender<SerializedBatchMessage>,
}

#[async_trait]
impl MessageHandler for WorkerReceiverHandler {
    async fn dispatch(&self, writer: &mut Writer, serialized: Bytes) -> Result<(), Box<dyn Error>> {
        // Reply with an ACK.
        let _ = writer.send(Bytes::from("Ack")).await;

        // Deserialize and parse the message.
        match bincode::deserialize(&serialized) {
            Ok(WorkerMessage::Batch(..)) => self
                .tx_processor
                .send(serialized.to_vec())
                .await
                .expect("Failed to send batch"),
            Ok(WorkerMessage::BatchRequest(missing, requestor)) => self
                .tx_helper
                .send((missing, requestor))
                .await
                .expect("Failed to send batch request"),

            Err(e) => warn!("Serialization error: {}", e),
        }
        Ok(())
    }
}

/// Defines how the network receiver handles incoming primary messages.
#[derive(Clone)]
struct PrimaryReceiverHandler {
    tx_synchronizer: Sender<PrimaryWorkerMessage>,
}

#[async_trait]
impl MessageHandler for PrimaryReceiverHandler {
    async fn dispatch(
        &self,
        _writer: &mut Writer,
        serialized: Bytes,
    ) -> Result<(), Box<dyn Error>> {
        // Deserialize the message and send it to the synchronizer.
        match bincode::deserialize(&serialized) {
            Err(e) => error!("Failed to deserialize primary message: {}", e),
            Ok(message) => self
                .tx_synchronizer
                .send(message)
                .await
                .expect("Failed to send transaction"),
        }
        Ok(())
    }
}


/// Defines how the network receiver handles incoming transactions.
#[derive(Clone)]
struct CrossShardReceiverHandler {
    tx_cross_shard_msg: Sender<CSMsg>,
}

#[async_trait]
impl MessageHandler for CrossShardReceiverHandler {
    async fn dispatch(
      &self,
      _writer: &mut Writer, 
      serialized: Bytes,
    ) -> Result<(), Box<dyn Error>> {

        // Deserialize the message and send it to the batch maker.
        match bincode::deserialize(&serialized) {
            Err(e) => error!("Failed to deserialize cross shard msg: {}", e),
            Ok(message) => self
                .tx_cross_shard_msg
                .send(message)
                .await
                .expect("Failed to send cross shard msg"),
        }

        // Give the change to schedule other tasks.
        tokio::task::yield_now().await;
        Ok(())
    }
}