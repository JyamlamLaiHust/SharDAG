use bytes::BytesMut;
use config::Committees;
use config::Export;
use crypto::{Digest, Hash, PublicKey, Signature, SignatureService};
use config::ShardId;
use ed25519_dalek::Digest as _;
use ed25519_dalek::Sha512;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt;


pub type Height = u64;
pub type Address = Vec<u8>;
pub type Amount = f64;


/// The message exchanged between workers from different shards.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum GeneralTransaction {
    TransferTx(Transaction), // original_tx or relayed_tx 原始交易或转发交易
    AggTx(AggTransaction), // 聚合交易
}

impl GeneralTransaction {

  // 获取交易计数器
  pub fn get_counter(&self) -> u64 {
    match self {
      GeneralTransaction::AggTx(_) => {0} // 聚合交易没有计数器
      GeneralTransaction::TransferTx(tx) => {
        tx.counter
      }
    }
  }

  // 判断是否为样本交易
  pub fn is_sample_tx(&self) -> bool {
      match self {
        GeneralTransaction::TransferTx(tx) => tx.sample == 0,
        _ => false,
      }
  }
  // 设置门限签名
  pub fn set_thres_sig(&mut self, thres_sig: Signature, source_shard: ShardId) {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.set_thres_sig(thres_sig, source_shard),
      GeneralTransaction::AggTx(tx) => tx.set_thres_sig(thres_sig),
    }
  }

  // return csmsg_id if the tx is csmsg
  pub fn get_csmsg_id(&self) -> Option<String> {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.get_csmsg_id(),
      GeneralTransaction::AggTx(tx) => tx.get_csmsg_id(),
    }
  }

  // 设置csmsg的序列号
  pub fn set_csmsg_seq(&mut self, csmsg_seq: u64) {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.set_csmsg_sequence(csmsg_seq),
      GeneralTransaction::AggTx(tx) => tx.set_csmsg_sequence(csmsg_seq),
    }
  }


  // 获取交易的payload摘要
  pub  fn get_payload_digest(&self) -> Digest {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.payload_hash.clone(),
      GeneralTransaction::AggTx(tx) => tx.payload_hash.clone(),
    }
  }

  // 计算打包的外部交易数
  pub fn count_packaged_external_tx(&self) -> u32 {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.count_packaged_external_tx(),
      GeneralTransaction::AggTx(_) => 0,
    }
  }

  // 计算外部交易数
  pub fn count_external_tx(&self) -> u32 {
    match self {
      GeneralTransaction::TransferTx(_) => 1,
      GeneralTransaction::AggTx(_) => 0,
    }
  }

  pub fn count_cs_tx(&self) -> u32 {
    match self {
      GeneralTransaction::TransferTx(tx) => {
        tx.count_cs_tx()
      },
      GeneralTransaction::AggTx(_) => 0,
    }
  }

  pub fn len(&self) -> usize {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.len(),
      GeneralTransaction::AggTx(tx) => tx.len(),
    }
  }

  pub fn get_digest(&self) -> Digest {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.digest(),
      GeneralTransaction::AggTx(agg_tx) => agg_tx.digest(),
    }
  }

  // TODO
  pub fn get_thres_sig(&self) -> Signature {
    match self {
      GeneralTransaction::AggTx(agg_tx) => agg_tx.thres_sig.clone(),
      GeneralTransaction::TransferTx(tx) => tx.cs_proof.last().unwrap().1.clone(),
    }
  }
 
  pub fn get_sample_tx_counter(&self) -> u64 {
    match self {
      GeneralTransaction::TransferTx(tx) => tx.counter,
      _ => 0,
    }
  }

  // TODO
  pub fn verify(&self, _all_committees: &Committees) -> bool {
    match self {
      _ => {return true}
    }
  }

  // TODO
  // verify the thresold sig of csmsg (relayed transfer tx or agg_tx)
  // return Some(tx_hash) if pass verification; else return None
  pub fn verify_cs_proof(&self) -> Option<Digest> {
    let tx_hash = self.get_digest();
    let _thres_sig = self.get_thres_sig();
    Some(tx_hash)
  }
}


// 原始交易数据
#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct RawTxOld {
  // pub block_number: usize,
  // pub tx_index: usize,
  pub sender: String, //20B
  pub receiver: String, 
  pub amount: Amount,
}

// 核心交易数据
#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct CoreTx {
  pub sample: u8, 
  pub counter: u64, 

  pub sender: Address, //20B
  pub receiver: Address, 
  pub amount: Amount,

  pub payload: Vec<RWSet>,
}


#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct  RWSet {
  pub addr: Address,
  pub value: Amount,
}


impl CoreTx {
  pub fn new(
    sample: u8,
    counter: u64,
    sender: Address,
    receiver: Address,
    amount: Amount,
    payload: Vec<RWSet>,
  ) -> Self {
    Self { sample, counter, sender, receiver, amount, payload}
  }
}

impl Hash for CoreTx {
  fn digest(&self) -> Digest {
      let mut hasher = Sha512::new();
      hasher.update(&self.sender);
      hasher.update(&self.receiver);
      hasher.update(self.amount.to_be_bytes());
      for rwset in self.payload.iter(){
        hasher.update(&rwset.addr);
        hasher.update(rwset.value.to_le_bytes());
      }
      Digest(hasher.finalize().as_slice()[..32].try_into().unwrap())
  }
}


// 聚合交易数据
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq)]
// rwsets executed by a shard 
pub struct Frame {
  pub shardid: ShardId,
  pub rwset: Vec<RWSet>,
}


#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AggTransaction {
  pub digest: Digest,
  pub source_shard: ShardId,
  pub thres_sig: Signature,
  pub csmsg_sequence: u64,

  pub payload_hash: Digest,
  pub payload_length: usize, // # of avatars
  pub payload: Vec<Frame>,

    // [source_shard] are updated in AggTransaction::new() by executor
    // [csmsg_sequence] is updated in CSMsg::new() by send_cs_msg 
    // [thres_sig] is updated in tx.set_thres_sig() by cs_msg_verifier
}

impl Export for AggTransaction {}

impl AggTransaction {
    pub fn len(&self) -> usize{
      let bytes = bincode::serialize(&self)
      .expect("Failed to serialize our own vote");
      bytes.len()
    }

    pub fn get_digest(&self) -> Digest {
      self.digest()
    }

    pub fn get_csmsg_id(&self) -> Option<String> {
      Some(format!("[{}-{}]", self.source_shard, self.csmsg_sequence))
    }

    pub fn set_thres_sig(&mut self, thres_sig: Signature) {
      self.thres_sig = thres_sig;
    }

    pub fn set_csmsg_sequence(&mut self, csmsg_seq: u64) {
      self.csmsg_sequence = csmsg_seq;
    }

    pub fn new(
      source_shard: ShardId,  
      payload: Vec<Frame>,
      payload_len: usize,
    ) -> Self {
      let mut tx = 
      Self {
        digest: Digest::default(),
        source_shard,
        thres_sig: Signature::default(),
        csmsg_sequence: 0,
        payload_hash: Digest::default(),
        payload_length: payload_len,
        payload, 
      };

      // calculate the agg tx payload hash
      let mut hasher = Sha512::new();
      let frame = tx.payload.get(0).unwrap();
      for rwset in frame.rwset.iter(){
        hasher.update(&rwset.addr);
        hasher.update(rwset.value.to_le_bytes());
      }
      tx.payload_hash = Digest(hasher.finalize().as_slice()[..32].try_into().unwrap());
      tx
    }
}

impl Hash for AggTransaction {
  fn digest(&self) -> Digest {
      let mut hasher = Sha512::new();
      hasher.update(self.source_shard.to_le_bytes());
      hasher.update(self.csmsg_sequence.to_le_bytes());
      hasher.update(self.payload_hash.to_vec());
      hasher.update(self.payload_length.to_be_bytes());
      Digest(hasher.finalize().as_slice()[..32].try_into().unwrap())
  }
}

impl fmt::Display for AggTransaction {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "[TX][source shard: {}, csmsg_sequence: {}, payload_length: {}]",
         self.source_shard, self.csmsg_sequence, self.payload_length
        )
  }
}


// 交易数据
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Transaction {
    pub sample: u8, // 0: sample tx; 1: Standard tx;
    pub counter: u64, // This counter identifies the tx. max: 18446744073709551615
    pub tx_hash: Digest,
    pub signature: Signature,  

    pub sender: Address, //20B
    pub receiver: Address, 
    pub amount: Amount,
    pub timestamp: u128,
    pub nonce: i32,

    // payload
    pub payload_hash: Digest,
    pub rwset_num: usize, // number of involved accounts (rwset)
    pub payload: Vec<Frame>,

    // used in broker 
    pub original_sender: Option<Address>,
    pub final_receiver: Option<Address>,

    // cross-shard info for relayed cross-shard tx
    pub involved_shard_num: usize, // involved shard num in a completely sharding archi
    pub step: usize, // the step-th frame
    pub cs_proof: Vec<(ShardId,Signature)>,
    pub source_shard: ShardId,
    pub csmsg_sequence: u64, 

    // [step] and [source_shard] are updated in tx.update_relay_info() by executor
    // [csmsg_sequence] is updated in CSMsg::new() by send_cs_msg 
    // [cs_proof] is updated in tx.set_thres_sig() by cs_msg_verifier

    pub padding: Vec<u8>,
}

impl Export for Transaction {}

impl Transaction {
    pub fn len(&self) -> usize{
      let bytes = bincode::serialize(&self)
      .expect("Failed to serialize our own vote");
      bytes.len()
    }

    pub fn is_sample_tx(&self) -> bool {
      self.sample == 0
    }

    pub fn get_csmsg_id(&self) -> Option<String> {
      if self.source_shard == ShardId::MAX {// not a csmsg
        None
      } else {
        Some(format!("[{}-{}]", self.source_shard, self.csmsg_sequence))
      }
    }


    // 交易在首次打包时统计
    pub fn count_packaged_external_tx(&self) -> u32 {
        if self.step == 0 { // step = 0 (tx2 in the Broker mechanism should not be counted)
          match &self.final_receiver {
            None => {1}
            Some(final_recv) => {
              if *self.receiver == *final_recv {0} // tx2 in the Broker mechanism
              else {1}
            }
          }
        } else {
          return 0;
        }
    }

    pub fn count_cs_tx(&self) -> u32 {
      if self.involved_shard_num > 1{ 
        return 1;
      } else {
        return 0;
      }
    }

    pub fn set_thres_sig(&mut self, thres_sig: Signature, source_shard: ShardId) {
      self.cs_proof.push((source_shard, thres_sig));      
    }

    pub fn set_csmsg_sequence(&mut self, csmsg_seq: u64) {
      self.csmsg_sequence = csmsg_seq;
    }

    pub fn get_digest(&self) -> Digest {
      self.digest()
    }

    // return the next shardid
    pub fn update_relay_info(&mut self, source_shard: ShardId) -> ShardId {
      self.step += 1;
      self.source_shard = source_shard;
      let next_shard = self.payload.get(self.step).unwrap().shardid;
      next_shard
    }

    pub fn new(
      sender: Address,
      recv: Address,
      amount: Amount,
      payload: Vec<Frame>,
      payload_len: usize,

      involved_shard_num: usize,
      tx_sample: u8,
      tx_counter: u64,
      timestamp: u128,

      original_sender: Option<Address>,
      final_receiver: Option<Address>,
    ) -> Self {
      let mut tx = 
      Self {
        sample: (tx_sample), counter: (tx_counter), 
        tx_hash: Digest::default(), signature: Signature::default(),
        sender: (sender), receiver: (recv), amount: (amount), 
        timestamp, nonce: 0, 
        
        payload,
        rwset_num: payload_len,
        payload_hash: Digest::default(),

        original_sender,
        final_receiver,

        step: 0,
        involved_shard_num,
        cs_proof: Vec::default(),
        source_shard: ShardId::MAX,
        csmsg_sequence: 0,
        padding: Vec::default()
      };
      // fullfill the padding
      if tx.len() < 512 {
        let mut res = BytesMut::default();
        res.resize(512-tx.len(), 0u8);
        tx.padding = res.to_vec();
      }

      // calculate the payload hash
      let mut hasher = Sha512::new();
      for frame in tx.payload.iter(){
        hasher.update(frame.shardid.to_be_bytes());
        for rwset in frame.rwset.iter(){
          hasher.update(&rwset.addr);
          hasher.update(rwset.value.to_le_bytes());
        }
      }
      tx.payload_hash = Digest(hasher.finalize().as_slice()[..32].try_into().unwrap());    

      tx
    }
}

impl Hash for Transaction {
  fn digest(&self) -> Digest {
      let mut hasher = Sha512::new();
      hasher.update(self.sample.to_le_bytes());
      hasher.update(self.counter.to_le_bytes());
      hasher.update(&self.sender);
      hasher.update(&self.receiver);
      hasher.update(self.amount.to_le_bytes());
      hasher.update(self.nonce.to_le_bytes());

      hasher.update(self.payload_hash.to_vec());
      hasher.update(self.rwset_num.to_be_bytes());
      hasher.update(self.step.to_be_bytes());

      Digest(hasher.finalize().as_slice()[..32].try_into().unwrap())
  }
}

impl fmt::Display for Transaction {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "[TX][sample: {}, counter: {}, sender: {:?}, receiver: {:?}, amount: {}]",
         self.sample, self.counter, self.sender, self.receiver, self.amount,
        )
  }
}


#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct CSMsg {
    // CSMsg header
    pub source_shard: ShardId,
    pub target_shard: ShardId,
    pub csmsg_sequence :u64, 

    pub tx: GeneralTransaction, // original_tx or agg_tx   
    pub inner_tx_hash: Digest, // serve as randomness to choose csmsg sender, reciver, and packagers.
    pub thres_sig: Signature,
 
    pub author: PublicKey,
    pub signature: Signature,
}

impl CSMsg {
  pub async fn new(
    source_shard: ShardId,
    target_shard: ShardId,
    csmsg_seq :u64,
    mut tx: GeneralTransaction, // original_tx or agg_tx

    author: &PublicKey, // author of the vote
    signature_service: &mut SignatureService, // used to set thres_sig
  ) -> Self {

    tx.set_csmsg_seq(csmsg_seq);
    let inner_tx_hash = tx.get_digest();
    let thres_sig = signature_service.request_signature(inner_tx_hash.clone()).await;
    
    let cs_msg = Self {
      source_shard, 
      target_shard, 
      csmsg_sequence: csmsg_seq,
      tx,
      inner_tx_hash,
      thres_sig,
      author: *author,
      signature:  Signature::default(),
    };
    cs_msg
  }

  pub async fn get_counter(
    &self
  ) -> u64 {
      self.tx.get_counter()
  }


  // set normal sig
  pub async fn set_sig(
    &mut self,
    signature_service: &mut SignatureService,
  ) {
    let signature = signature_service.request_signature(self.digest()).await;
    self.signature = signature;
  }
  
  pub fn len(&self) -> usize {
    let bytes = bincode::serialize(&self)
    .expect("Failed to serialize our own vote");
    bytes.len()
  }
  
  pub fn get_digest(&self) -> Digest {
    self.digest()
  }

  /// TODO verify thres_sig
  pub fn verify(&self, _all_committees: &Committees) -> bool {
    return true;
  }

}

impl Hash for CSMsg {
  fn digest(&self) -> Digest {
      let mut hasher = Sha512::new();
      hasher.update(self.source_shard.to_le_bytes());
      hasher.update(self.target_shard.to_le_bytes());
      hasher.update(self.csmsg_sequence.to_le_bytes());

      Digest(hasher.finalize().as_slice()[..32].try_into().unwrap())
  }
}

impl fmt::Display for CSMsg {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "[AD][source_shard: {}, target_shard: {}, csmsg_sequence: {}]",
         self.source_shard, self.target_shard, self.csmsg_sequence)
  }
}