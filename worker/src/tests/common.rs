use crate::messages::{AAReq, AARep};
use crate::state::Account;
use crate::{Transaction, GeneralTransaction, AccountState, AccountShard};
// Copyright(C) Facebook, Inc. and its affiliates.
use crate::batch_maker::Batch;
use crate::worker::WorkerMessage;
use bytes::Bytes;
use config::{Authority, Committees, Committee, PrimaryAddresses, WorkerAddresses, ShardId};
use crypto::{generate_keypair, Digest, PublicKey, SecretKey, Signature, SignatureService};
use ed25519_dalek::Digest as _;
use ed25519_dalek::Sha512;
use futures::sink::SinkExt as _;
use futures::stream::StreamExt as _;
use primary::Header;
use rand::rngs::StdRng;
use rand::SeedableRng as _;
use std::convert::TryInto as _;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use std::collections::{HashMap, BTreeMap, BTreeSet};

// Fixture
pub fn keys(shardid: ShardId) -> Vec<(PublicKey, SecretKey)> {
      let mut rng = StdRng::from_seed([shardid as u8; 32]);
      (0..4).map(|_| generate_keypair(&mut rng)).collect()
}

// Fixture
pub fn committee(shardid: ShardId) -> Committee {
    Committee {
        authorities: keys(shardid)
            .iter()
            .enumerate()
            .map(|(i, (id, _))| {
                let primary = PrimaryAddresses {
                    primary_to_primary: format!("127.0.0.1:{}", 100 + i).parse().unwrap(),
                    worker_to_primary: format!("127.0.0.1:{}", 200 + i).parse().unwrap(),
                };
                let workers = vec![(
                    0,
                    WorkerAddresses {
                        primary_to_worker: format!("127.0.0.1:{}", 300 + i).parse().unwrap(),
                        transactions: format!("127.0.0.1:{}", 400 + i).parse().unwrap(),
                        worker_to_worker: format!("127.0.0.1:{}", 500 + i).parse().unwrap(),
                        cross_shard_worker: format!("127.0.0.1:{}", 600 + i).parse().unwrap(),
                    },
                )]
                .iter()
                .cloned()
                .collect();
                (
                    *id,
                    Authority {
                        stake: 1,
                        primary,
                        workers,
                    },
                )
            })
            .collect(),
    }
}

// Fixture.
pub fn committee_with_base_port(base_port: u16, shardid: ShardId) -> Committee {
    let mut committee = committee(shardid);
    for authority in committee.authorities.values_mut() {
        let primary = &mut authority.primary;

        let port = primary.primary_to_primary.port();
        primary.primary_to_primary.set_port(base_port + port);

        let port = primary.worker_to_primary.port();
        primary.worker_to_primary.set_port(base_port + port);

        for worker in authority.workers.values_mut() {
            let port = worker.primary_to_worker.port();
            worker.primary_to_worker.set_port(base_port + port);

            let port = worker.transactions.port();
            worker.transactions.set_port(base_port + port);

            let port = worker.worker_to_worker.port();
            worker.worker_to_worker.set_port(base_port + port);

            let port = worker.cross_shard_worker.port();
            worker.cross_shard_worker.set_port(base_port + port);
        }
    }
    committee
}

// pub fn committees(committee_0: Committee, committee_1: Committee) -> Committees {  
//   let shards: HashMap<ShardId, Committee> = vec![(0, committee_0), (1, committee_1)].iter().cloned().collect();
//   let committees = Committees{shards };
//   committees
// }

pub fn committees(base_port: u16, shard_num: u32) -> Committees {

  let mut shards: HashMap<ShardId, Committee> = HashMap::new();
  for i in 0..shard_num {
    shards.insert(i,  committee_with_base_port(base_port+ i as u16 * 1000, i));
  }
  let committees = Committees{shards, shard_num, shard_size: 4, client  };
  committees

}

pub fn intra_transaction() -> Transaction {
  //vec![0; 100]
  let mut tx = Transaction::default();
  tx.sender = String::from("0x3f5ce5fbfe3e9af3971dd833d26ba9b5c936f0be");
  tx.receiver = String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4");
  tx
}

// Fixture
pub fn transaction() -> Transaction {
    //vec![0; 100]
    let mut tx = Transaction::default();
    tx.sender = String::from("0x3f5ce5fbfe3e9af3971dd833d26ba9b5c936f0be");
    tx.receiver = String::from("0xc1c03b8fa379f6ca8a11833f9c2e0775a9715907");
    tx
}

pub fn transaction_for_aa() -> Transaction {
  let mut tx = Transaction::default();
  tx.sender = String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4");
  tx.receiver = String::from("0x3f5ce5fbfe3e9af3971dd833d26ba9b5c936f0be");
  tx.amount = 10.0;// sender.balance = 5.0
  tx
}

pub fn transaction_aa_req() -> AAReq {
  AAReq { 
    primary_shard_id: 1, height: 10, // shard 1在height 10处生成aa_req
    account_addr: String::from("0xc1c03b8fa379f6ca8a11833f9c2e0775a9715907"), 
    tem_shard_ids: HashMap::from([(0, 2)]), // shard 0在height 2处创建了tem acc
    author: PublicKey::default(), signature: Signature::default() 
  }
}

pub fn transaction_aa_rep() -> AARep { 
  AARep { 
    tem_shard_id: 0, height: 16, primary_shard_id: 1, 
    initial_aa_req_height: 10, 
    account_addr: String::from("0xc1c03b8fa379f6ca8a11833f9c2e0775a9715907"), 
    tem_acc_balance: 0.0, 
    author: PublicKey::default(), signature: Signature::default()  
  }
}




pub enum TestTxType {
    TxForAccAggregation,
    TxForCreateTemAcc,
    TxForAAReq,
    TxForAARep
}


// Fixture
pub fn batch(tx_type: TestTxType) -> Batch {    
    match tx_type {
      TestTxType::TxForCreateTemAcc => {
        Batch { 
          ad_list: Vec::new(), 
          tx_list: vec![GeneralTransaction::TransferTx(transaction()), GeneralTransaction::TransferTx(transaction())] 
        }        
      },
      TestTxType::TxForAccAggregation => {
        Batch { 
          ad_list: Vec::new(), 
          tx_list: vec![GeneralTransaction::TransferTx(transaction_for_aa())]
        }
      },
      TestTxType::TxForAAReq => {
        Batch { 
          ad_list: Vec::new(), 
          tx_list: vec![GeneralTransaction::AAReq(transaction_aa_req())]
        }
      },
      TestTxType::TxForAARep => {
        Batch { 
          ad_list: Vec::new(), 
          tx_list: vec![GeneralTransaction::AARep(transaction_aa_rep())]
        }
      },
    }
}

pub fn header(tx_type: TestTxType) -> Header {
  Header { 
    author: PublicKey::default(),     
    round: 0, 
    payload: BTreeMap::from([(batch_digest(tx_type), 0)]), 
    parents: BTreeSet::default(), 
    id: Digest::default(), 
    signature: Signature::default()  
  }
}


// Fixture
pub fn serialized_batch(tx_type: TestTxType) -> Vec<u8> {
    let message = WorkerMessage::Batch(batch(tx_type));
    bincode::serialize(&message).unwrap()
}

// Fixture
pub fn batch_digest(tx_type: TestTxType) -> Digest {
    Digest(
        Sha512::digest(&serialized_batch(tx_type)).as_slice()[..32]
            .try_into()
            .unwrap(),
    )
}

// Fixture
pub fn listener(address: SocketAddr, expected: Option<Bytes>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(&address).await.unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let transport = Framed::new(socket, LengthDelimitedCodec::new());
        let (mut writer, mut reader) = transport.split();
        match reader.next().await {
            Some(Ok(received)) => {
                writer.send(Bytes::from("Ack")).await.unwrap();
                if let Some(expected) = expected {
                    assert_eq!(received.freeze(), expected);
                }
            }
            _ => panic!("Failed to receive network message"),
        }
    })
}


// 共有2个分片，这里返回shard 0 的状态
pub fn account_state() -> (AccountState, AccountShard) {

  let account1 = Account {
    //nonce: 0,
    balance: 0.0,
    create_height: 2,// 模拟在height 2处为该账户生成了tem acc
  };
  let account2 = Account {
    //nonce: 0,
    balance: 0.0,
    create_height: 0,
  };
  let account3 = Account {
    //nonce: 0,
    balance: 0.0,
    create_height: 0,
  };  
  let mut account_states = HashMap::new();
  account_states.insert(String::from("0xc1c03b8fa379f6ca8a11833f9c2e0775a9715907"), account1);//在测试process_aa_req时需要插入这一条
  account_states.insert(String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4"), account2);
  account_states.insert(String::from("0x3f5ce5fbfe3e9af3971dd833d26ba9b5c936f0be"), account3);

  let mut account_shard = HashMap::new();
  account_shard.insert(String::from("0xc1c03b8fa379f6ca8a11833f9c2e0775a9715907"), 1);
  account_shard.insert(String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4"), 0);
  account_shard.insert(String::from("0x3f5ce5fbfe3e9af3971dd833d26ba9b5c936f0be"), 0);
  
  (AccountState {
    account_states,
  },
  AccountShard {
    account_shard,
  })
}