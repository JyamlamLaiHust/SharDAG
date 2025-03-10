// Copyright(C) Facebook, Inc. and its affiliates.
use super::*;
use crate::common::{batch_digest, committee_with_base_port, keys, listener, TestTxType};
use std::fs;
use tokio::sync::mpsc::channel;

#[tokio::test]
async fn synchronize() {
    let (tx_message, rx_message) = channel(1);
    let (tx_process, rx_process) = channel(1);

    let mut keys = keys(0);
    let (name, _) = keys.pop().unwrap();
    let id = 0;
    let committee = committee_with_base_port(9_000, 0);

    // Create a new test store.
    let path = ".db_test_synchronize";
    let _ = fs::remove_dir_all(path);
    let store = Store::new(path).unwrap();

    // Spawn a `Synchronizer` instance.
    Synchronizer::spawn(
        name,
        id,
        committee.clone(),
        store.clone(),
        /* gc_depth */ 50, // Not used in this test.
        /* sync_retry_delay */ 1_000_000, // Ensure it is not triggered.
        /* sync_retry_nodes */ 3, // Not used in this test.
        rx_message,
        tx_process,
    );

    // Spawn a listener to receive our batch requests.
    let (target, _) = keys.pop().unwrap();
    let address = committee.worker(&target, &id).unwrap().worker_to_worker;
    let missing = vec![batch_digest(TestTxType::TxForCreateTemAcc)];
    let message = WorkerMessage::BatchRequest(missing.clone(), name);
    let serialized = bincode::serialize(&message).unwrap();
    let handle = listener(address, Some(Bytes::from(serialized)));

    // Send a sync request.
    let message = PrimaryWorkerMessage::Synchronize(missing, target);
    tx_message.send(message).await.unwrap();

    // Ensure the target receives the sync request.
    assert!(handle.await.is_ok());
}
