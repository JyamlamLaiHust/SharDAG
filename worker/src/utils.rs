use std::{mem, collections::HashMap};
use config::ShardId;
use crypto::Digest;
use permutation_iterator::Permutor;
use crate::{RWSet, Address, messages::Amount};



pub trait AllocatedSize {
  fn allocated_size(&self) -> usize;
}

impl AllocatedSize for ShardId {
  fn allocated_size(&self) -> usize {
      0
  }
}

impl AllocatedSize for String {
  fn allocated_size(&self) -> usize {
      self.capacity()
  }
}

impl AllocatedSize for Address {
  fn allocated_size(&self) -> usize {
      self.capacity()
  }
}

impl AllocatedSize for Amount {
  fn allocated_size(&self) -> usize {
      0
  }
}

impl AllocatedSize for RWSet {
  fn allocated_size(&self) -> usize {
    let directly_owned = mem::size_of::<RWSet>();
    let transitively_owned = self.addr.allocated_size();
    directly_owned + transitively_owned
  }
}

impl AllocatedSize for Vec<RWSet> {
  fn allocated_size(&self) -> usize {
    let directly_owned = mem::size_of::<Vec<RWSet>>();
    let transitively_owned: usize = self
    .iter()
    .map(|ad_item|ad_item.allocated_size())
    .sum();
    directly_owned + transitively_owned
  }    
}

impl AllocatedSize for HashMap<ShardId, HashMap<Address, Amount>> {
  fn allocated_size(&self) -> usize {
      // every element in the map directly owns its key and its value
      const ELEMENT_SIZE: usize = std::mem::size_of::<ShardId>() + std::mem::size_of::<HashMap<Address, Amount>>();
      // println!("{}", ELEMENT_SIZE);

      // directly owned allocation
      // NB: self.capacity() may be an underestimate, see its docs
      // NB: also ignores control bytes, see hashbrown implementation
      let directly_owned = self.capacity() * ELEMENT_SIZE;
      // println!("{}", directly_owned);
      // transitively owned allocations
      let transitively_owned: usize = self
          .iter()
          .map(|(key, val)| 
              {
                // println!("{}", key.allocated_size() + val.allocated_size());
                key.allocated_size() + val.allocated_size()
              }
          )
          .sum();
      // println!("{}", transitively_owned);
      directly_owned + transitively_owned
  }
}



impl AllocatedSize for HashMap<Address, Amount> {
  fn allocated_size(&self) -> usize {
      // every element in the map directly owns its key and its value
      const ELEMENT_SIZE: usize = std::mem::size_of::<Address>() + std::mem::size_of::<Amount>();
      // println!("{}", ELEMENT_SIZE);

      // directly owned allocation
      // NB: self.capacity() may be an underestimate, see its docs
      // NB: also ignores control bytes, see hashbrown implementation
      let directly_owned = self.capacity() * ELEMENT_SIZE;
      // println!("{}", directly_owned);
      // transitively owned allocations
      let transitively_owned: usize = self
          .iter()
          .map(|(key, val)| 
              {
                // println!("{}", key.allocated_size() + val.allocated_size());
                key.allocated_size() + val.allocated_size()
              }
          )
          .sum();
      // println!("{}", transitively_owned);
      directly_owned + transitively_owned
  }
}


pub fn shuffle_node_id_list(shard_size: usize, msg_hash: &Digest) -> Vec<usize>{
  let permutor = Permutor::new_with_slice_key(shard_size as u64, msg_hash.0); 
  let mut candi_node_id_list: Vec<usize> = Vec::new();
  for index in permutor {
    candi_node_id_list.push(index as usize);
  }
  return candi_node_id_list
}

pub fn random_select_brokers(brokers: Vec<Vec<u8>>, total_num: usize, target_num: usize, seed: u8) -> Vec<Vec<u8>> {
  let mut selected_brokers: Vec<Vec<u8>> = Vec::default();

  let key: Digest = Digest([seed; 32]);
  let indexs = shuffle_node_id_list(total_num, &key);
  let indexs = &indexs[0..target_num];

  for index in indexs {
      let broker = brokers[*index].clone();
      selected_brokers.push(broker);
  }
  selected_brokers
}