use rocksdb::{DBWithThreadMode, SingleThreaded};
use sp_std::prelude::*;



type Key = Vec<u8>;
type Value = Vec<u8>;



#[derive(Debug)]
pub struct DiskDB {
  data: DBWithThreadMode<SingleThreaded>,
}

impl DiskDB {
	pub fn new(path: &str) -> Self {
      let db = rocksdb::DB::open_default(path);
      let data = match db {
        Ok(file) => file,
        Err(error) => {
            panic!("Problem opening the file: {:?}", error)
        },
      };
      Self { data }
  }


  pub fn get(&self, key: &[u8]) -> Option<Value> {
    // let (sender, receiver) = oneshot::channel();
    // if let Err(e) = executor::block_on(self.channel.send(StoreCommand::Read(key.to_vec(), sender))) {
    //     panic!("Failed to send Read command to store: {}", e);
    // }
    // let res =executor::block_on(receiver)
    //     .expect("Failed to receive reply to Read command from store");
    // return res;
    self.data.get(&key).unwrap()
  }

  pub fn insert(&mut self, key: Key, value: Value) {
    let _ = self.data.put(&key, &value);
    // if let Err(e) = executor::block_on(self.channel.send(StoreCommand::Write(key, value))) {
    //     panic!("Failed to send Write command to store: {}", e);
    // }
  }

  pub fn remove(&mut self, key: &[u8]) {
    // if let Err(e) = executor::block_on(self.channel.send(StoreCommand::Remove(key.to_vec()))) {
    //     panic!("Failed to send Read command to store: {}", e);
    // }
    let _ = self.data.delete(&key);
  }

  /// Insert a batch of data into the cache.
  pub fn insert_batch(&mut self, keys: Vec<Key>, values: Vec<Value>) {
    for i in 0..keys.len() {
      let key = keys[i].clone();
      let value = values[i].clone();
      self.insert(key, value);
    }
  }

	/// Remove a batch of data into the cache.
	pub fn remove_batch(&mut self, keys: &[Key]) {
		for key in keys {
      self.remove(key);
		}
	}

}

