use hex::FromHex;
use log::info;
use worker::{CoreTx, BrokerItem, random_select_brokers};
use csv::DeserializeRecordsIter;
use std::{collections::HashMap, fs::File};



pub const BROKER_NUM: usize = 40; // (rounds)


// broker addr manager
pub struct BrokerManager {
  broker_addrs: Vec<Vec<u8>>,
  pub tx1s: HashMap<u64, Tx1Msg>, // key is tx.counter
}


#[derive(Debug)]
pub struct Tx1Msg {
  pub core_tx: CoreTx,
  pub broker: Vec<u8>,
}

impl BrokerManager {
  // load brokers from brokers_file
  pub fn new(brokers_file: String, epoch: usize) -> Self {
    // info!("brokers_file: {:?}", brokers_file);

    let mut broker_addrs: Vec<Vec<u8>> = Vec::default();

    let mut reader = csv::Reader::from_path(brokers_file).unwrap();
    let mut state_iter: DeserializeRecordsIter<File, BrokerItem> = reader.deserialize().into_iter();
    while let Some(Ok(broker_item)) = state_iter.next(){
      let broker = broker_item.account;
      broker_addrs.push(Vec::from_hex(&broker[2..]).unwrap());
    }

    // random select BROKER_NUM brokers
    broker_addrs = random_select_brokers(broker_addrs, 100, BROKER_NUM, epoch as u8);

    // broker_addrs.push(Vec::from_hex("066BC6836d8AFe1fD20F85cE6a9A9489d0602239").unwrap());
    info!("Load {:?} brokers!", broker_addrs.len());

    Self {
      broker_addrs, 
      tx1s: HashMap::default(),
    }
  }

  pub fn add_tx1(&mut self, core_tx: CoreTx, tx_counter: u64, broker: Vec<u8>) {
    let tx1_msg = Tx1Msg{core_tx, broker};
    self.tx1s.insert(tx_counter, tx1_msg);
  }

  pub fn delete_tx1(&mut self, tx_counter: u64) -> Option<Tx1Msg> {
    self.tx1s.remove(&tx_counter)
  }
  

  pub fn is_broker(&self, addr: &Vec<u8>) -> bool {
    self.broker_addrs.contains(addr)
  }

  pub fn get_broker(&self) -> Vec<u8> {
    let broker = self.broker_addrs.get(0).unwrap();
    broker.clone()
  }
}



