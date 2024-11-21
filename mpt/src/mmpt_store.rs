use std::collections::HashMap;
use sp_std::prelude::*;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::oneshot;
use async_trait::async_trait;
use crate::mem_trie::MemMerklePatriciaTrie;
use crate::mpt_store::{StoreCommand, RootHash};
use crate::{TrieResult, Trie, Proof, Key, Value, MemoryDB, MPTStoreTrait};
use std::sync::Arc;


#[derive(Clone)]
pub struct MMPTStore {
  channel: Sender<StoreCommand>,
}

impl MMPTStore {
	pub fn new() -> Self {

    // Make the data store.
    let memdb = Arc::new(MemoryDB::new());
		let mut state_trie = MemMerklePatriciaTrie::new(memdb);

    let (tx, mut rx) = channel(200);
    tokio::spawn(async move {  
          while let Some(command) = rx.recv().await {
              match command {
                  StoreCommand::Get(key, sender) => {
                    let response = state_trie.get(&key);
                    let _ = sender.send(response);
                  }
                  StoreCommand::Contains(key, sender) => {
                    let response = state_trie.contains(&key);
                    let _ = sender.send(response);
                  }                 
                  StoreCommand::Insert(key, value, sender) => {
                      let response = state_trie.insert(key, value);
                      let _ = sender.send(response);
                  }                  
                  StoreCommand::Remove(key, sender) => {
                    let response = state_trie.remove(&key);
                    let _ = sender.send(response);
                  }
                  StoreCommand::Root(sender) => {
                    let response = state_trie.root();
                    let _ = sender.send(response);
                  }
                  StoreCommand::GetProof(key, sender) => {
                    let response = state_trie.get_proof(&key);
                    let _ = sender.send(response);
                  }
                  StoreCommand::GetProofBatch(accs, sender) => {
                    let mut acc_proof : HashMap<Key, Proof> = HashMap::default();
                    for addr in accs {
                      let proof = state_trie.get_proof(&addr).unwrap();
                      acc_proof.insert(addr.clone(), proof);
                    }
                    let _ = sender.send(Ok(acc_proof));
                  }
                  StoreCommand::RemoveBatch(accs, sender) => {
                    let mut removed_acc_nums = 0;
                    for addr in accs {
                      if state_trie.remove(&addr).unwrap() {
                        removed_acc_nums += 1;
                      }
                    }
                    let _ = sender.send(Ok(removed_acc_nums));
                  }
                  StoreCommand::InsertBatch(accs, sender) => {
                    let mut inserted_acc_nums = 0;
                    for (addr, value) in accs {
                      if state_trie.insert(addr, value).unwrap(){
                        inserted_acc_nums += 1;
                      }
                    }
                    let _ = sender.send(Ok(inserted_acc_nums));
                  }
              }
          }
      });
      Self { channel: tx }
  }

}

#[async_trait]
impl MPTStoreTrait for MMPTStore {

  async fn get(&mut self, key: &[u8]) -> TrieResult<Option<Vec<u8>>> {
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::Get(key.to_vec(), sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }

  async fn contains(&mut self, key: &[u8]) -> TrieResult<bool> {
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::Contains(key.to_vec(), sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }

  async fn insert(&mut self, key: Key, value: Value) -> TrieResult<bool>{   
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::Insert(key, value, sender)).await {
        panic!("Failed to send Write command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }

  async fn remove(&mut self, key: &[u8]) -> TrieResult<bool>{
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::Remove(key.to_vec(), sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }

  async fn root(&mut self) -> TrieResult<RootHash>{    
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::Root(sender)).await {
        panic!("Failed to send Write command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }


  async fn get_proof(&mut self, key: &[u8]) -> TrieResult<Proof> {
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::GetProof(key.to_vec(), sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }

  async fn get_proof_batch(&mut self, accs: Vec<Key>) -> TrieResult<HashMap<Key, Proof>>{
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::GetProofBatch(accs, sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }


  async fn remove_batch(&mut self, accs: Vec<Key>) -> TrieResult<i32>{
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::RemoveBatch(accs, sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }

  async fn insert_batch(&mut self, accs: HashMap<Key, Value>) -> TrieResult<i32>{
    let (sender, receiver) = oneshot::channel();
    if let Err(e) = self.channel.send(StoreCommand::InsertBatch(accs, sender)).await {
        panic!("Failed to send Read command to store: {}", e);
    }
    receiver
    .await
    .expect("Failed to receive reply to Read command from store")
  }
}

