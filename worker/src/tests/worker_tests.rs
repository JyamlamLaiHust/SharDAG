// Copyright(C) Facebook, Inc. and its affiliates.
use super::*;
use crate::common::{batch_digest, committee_with_base_port, keys, listener, transaction, committees, account_state, TestTxType};
use network::SimpleSender;
use primary::WorkerPrimaryMessage;
use std::fs;


#[tokio::test]
async fn handle_clients_transactions() {
    let (name, secret) = keys(0).pop().unwrap();
    let id = 0;
    let shardnum = 2;

    let committee = committee_with_base_port(11_000, 0);
    let committee_1 = committee_with_base_port(12_000, 1);

    let parameters = Parameters {
        batch_size: 200, // Two transactions.
        ..Parameters::default()
    };

    let shardid = 0;
    let committees = committees(11_000, shardnum);
    // 
    let shard_num = 1;

    // Create a new test store.
    let path = ".db_test_handle_clients_transactions";
    let _ = fs::remove_dir_all(path);
    let store = Store::new(path).unwrap();

    let (account_state, account_shard) = account_state(); 


    // Spawn a `Worker` instance.
    Worker::spawn(name, secret, id, committee.clone(), parameters, store, shard_num, shardid,committees, account_state, account_shard);

    // Spawn a network listener to receive our batch's digest.
    let primary_address = committee.primary(&name).unwrap().worker_to_primary;
    let expected = bincode::serialize(&WorkerPrimaryMessage::OurBatch(batch_digest(TestTxType::TxForCreateTemAcc), id)).unwrap();
    let handle = listener(primary_address, Some(Bytes::from(expected)));

    // Spawn enough workers' listeners to acknowledge our batches.
    for (_, addresses) in committee.others_workers(&name, &id) {
        let address = addresses.worker_to_worker;
        let _ = listener(address, /* expected */ None);
    }

    // Send enough transactions to create a batch.
    let mut network = SimpleSender::new();
    let address = committee.worker(&name, &id).unwrap().transactions;

    let general_tx = GeneralTransaction::TransferTx(transaction());
    let bytes = bincode::serialize(&general_tx).expect("Failed to serialize our transaction");

    network.send(address, Bytes::from(bytes.clone())).await;
    network.send(address, Bytes::from(bytes)).await;

    // Ensure the primary received the batch's digest (ie. it did not panic).
    assert!(handle.await.is_ok());
}
