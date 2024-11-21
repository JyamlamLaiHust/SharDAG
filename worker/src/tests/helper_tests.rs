// Copyright(C) Facebook, Inc. and its affiliates.
use super::*;
use crate::common::{batch_digest, committee_with_base_port, keys, listener, serialized_batch, TestTxType};
use std::fs;
use tokio::sync::mpsc::channel;

#[tokio::test]
async fn batch_reply() {
    let (tx_request, rx_request) = channel(1);
    let (requestor, _) = keys(0).pop().unwrap();
    let id = 0;
    let committee = committee_with_base_port(8_000, 0);

    // Create a new test store.
    let path = ".db_test_batch_reply";
    let _ = fs::remove_dir_all(path);
    let mut store = Store::new(path).unwrap();

    // Add a batch to the store.
    store
        .write(batch_digest(TestTxType::TxForCreateTemAcc).to_vec(), serialized_batch(TestTxType::TxForCreateTemAcc))
        .await;

    // Spawn an `Helper` instance.
    Helper::spawn(id, committee.clone(), store, rx_request);

    // Spawn a listener to receive the batch reply.
    let address = committee.worker(&requestor, &id).unwrap().worker_to_worker;
    let expected = Bytes::from(serialized_batch(TestTxType::TxForCreateTemAcc));
    let handle = listener(address, Some(expected));

    // Send a batch request.
    let digests = vec![batch_digest(TestTxType::TxForCreateTemAcc)];
    tx_request.send((digests, requestor)).await.unwrap();

    // Ensure the requestor received the batch (ie. it did not panic).
    assert!(handle.await.is_ok());
}
