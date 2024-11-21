// Copyright(C) Facebook, Inc. and its affiliates.
use super::*;
use crate::common::transaction;
use tokio::sync::mpsc::channel;

#[tokio::test]
async fn make_batch() {
    let (tx_transaction, rx_transaction) = channel(1);
    let (tx_message, mut rx_message) = channel(1);
    let dummy_addresses = vec![(PublicKey::default(), "127.0.0.1:0".parse().unwrap())];

    // Spawn a `BatchMaker` instance.
    BatchMaker::spawn(
        /* max_batch_size */ 200, // 收到两笔交易就触发打包batch
        /* max_batch_delay */ 1_000_000, // Ensure the timer is not triggered.
        rx_transaction,
        tx_message,
        /* workers_addresses */ dummy_addresses,
    );

    // Send enough transactions to seal a batch.
    tx_transaction.send(GeneralTransaction::TransferTx(transaction())).await.unwrap();
    tx_transaction.send(GeneralTransaction::TransferTx(transaction())).await.unwrap();

    // Ensure the batch is as expected.
    //let expected_batch = vec![transaction(), transaction()];
    let expected_batch = Batch { 
      ad_list: Vec::new(), 
      tx_list: vec![GeneralTransaction::TransferTx(transaction()), GeneralTransaction::TransferTx(transaction())] 
    };

    let QuorumWaiterMessage { batch, handlers: _ } = rx_message.recv().await.unwrap();
    match bincode::deserialize(&batch).unwrap() {
        WorkerMessage::Batch(batch) => assert_eq!(batch.tx_list.len(), expected_batch.tx_list.len()),
        _ => panic!("Unexpected message"),
    }
}

#[tokio::test]
async fn batch_timeout() {
    let (tx_transaction, rx_transaction) = channel(1);
    let (tx_message, mut rx_message) = channel(1);
    let dummy_addresses = vec![(PublicKey::default(), "127.0.0.1:0".parse().unwrap())];

    // Spawn a `BatchMaker` instance.
    BatchMaker::spawn(
        /* max_batch_size */ 200,
        /* max_batch_delay */ 50, // Ensure the timer is triggered.
        rx_transaction,
        tx_message,
        /* workers_addresses */ dummy_addresses,
    );

    // Do not send enough transactions to seal a batch..
    tx_transaction.send(GeneralTransaction::TransferTx(transaction())).await.unwrap();

    // Ensure the batch is as expected.
    let expected_batch = vec![transaction()];

    let expected_batch = Batch { 
      ad_list: Vec::new(), 
      tx_list: vec![GeneralTransaction::TransferTx(transaction())] 
    };
    
    let QuorumWaiterMessage { batch, handlers: _ } = rx_message.recv().await.unwrap();
    match bincode::deserialize(&batch).unwrap() {
        WorkerMessage::Batch(batch) => assert_eq!(batch.tx_list.len(), expected_batch.tx_list.len()),
        _ => panic!("Unexpected message"),
    }
}
