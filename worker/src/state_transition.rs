use std::collections::HashMap;
use log::debug;
use crate::{StateStore, Address, state_store::Account, RWSet, INIT_BALANCE};

pub struct StateTransition{
  pub store: Box<dyn StateStore + Send>,
}

impl StateTransition {
  pub fn new(
    store: Box<dyn StateStore + Send>,
  ) -> Self {
    Self { store }
  }

  pub async fn get_latest_states(&mut self, rwset: &Vec<RWSet>) -> HashMap<Address,Account> {
    let mut temp_states:HashMap<Address,Account> = HashMap::new();
    for rw in rwset {
      let value = self.store.get(&rw.addr).await;
      match value {
          Some(value) => {
            let acc: Account = bincode::deserialize(&value).unwrap();
            temp_states.insert(rw.addr.clone(), acc);
          },
          None => {  // crate a new account
            debug!("This account appears for the first time");
            temp_states.insert(rw.addr.clone(), Account{nonce: 0, balance: INIT_BALANCE});
          }
      }
    }
    temp_states
  }
  
  pub async fn apply_new_states(&mut self, new_states: HashMap<Address,Account>) {
    for (addr, account) in new_states {
      let serialized = bincode::serialize(&account).expect("Failed to serialize account");
      self.store.insert(addr, serialized).await;
    }
  }
}




