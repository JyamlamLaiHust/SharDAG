// Copyright(C) Facebook, Inc. and its affiliates.
use super::*;
use std::fs;

#[tokio::test]
async fn create_store() {
    // Create new store.
    let path = ".db_test_create_store";  // 定义存储路径
    let _ = fs::remove_dir_all(path); // 确保路径不存在，删除任何现有数据
    let store = Store::new(path); // 创建存储实例
    assert!(store.is_ok()); // 验证存储实例创建完成
}

#[tokio::test]
async fn read_write_value() {
    // Create new store.
    let path = ".db_test_read_write_value";
    let _ = fs::remove_dir_all(path);
    let mut store = Store::new(path).unwrap();

    // Write value to the store.
    // 向存储写入一个键值对
    let key = vec![0u8, 1u8, 2u8, 3u8];
    let value = vec![4u8, 5u8, 6u8, 7u8];
    store.write(key.clone(), value.clone()).await; // 异步写入键值对

    // Read value.
    // 从存储读取值
    let result = store.read(key).await; // 异步读取键对应的值
    assert!(result.is_ok()); // 确保读取操作成功
    let read_value = result.unwrap(); // 读取结果
    assert!(read_value.is_some()); // 确保值存在
    assert_eq!(read_value.unwrap(), value); // 验证读取值和写入值相等
}

#[tokio::test]
async fn read_unknown_key() {
    // Create new store.
    let path = ".db_test_read_unknown_key";
    let _ = fs::remove_dir_all(path);
    let mut store = Store::new(path).unwrap();

    // Try to read unknown key.
    // 尝试读取一个不存在的值
    let key = vec![0u8, 1u8, 2u8, 3u8];
    let result = store.read(key).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn read_notify() {
    // Create new store.
    let path = ".db_test_read_notify";
    let _ = fs::remove_dir_all(path);
    let mut store = Store::new(path).unwrap();

    // Try to read a kew that does not yet exist. Then write a value
    // for that key and check that notify read returns the result.
    let key = vec![0u8, 1u8, 2u8, 3u8];
    let value = vec![4u8, 5u8, 6u8, 7u8];

    // Try to read a missing value.
    let mut store_copy = store.clone();
    let key_copy = key.clone();
    let value_copy = value.clone();
    let handle = tokio::spawn(async move {
        match store_copy.notify_read(key_copy).await {
            Ok(v) => assert_eq!(v, value_copy),
            _ => panic!("Failed to read from store"),
        }
    });

    // Write the missing value and ensure the handle terminates correctly.
    // 写入缺失的值并确保任务正确终止
    store.write(key, value).await; //异步写入键值对
    assert!(handle.await.is_ok());
}
