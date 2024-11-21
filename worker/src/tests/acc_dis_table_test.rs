use std::vec;

use super::{ADTStore, Item};








/// 测试方法：先insert一些item, 然后get_ad

#[tokio::test]
async fn update_adt() {

  let shardid = 0;
  let shardnum = 4;
  let mut adt_store = ADTStore::new(shardnum);


  // 节点位于shard: 0, 是"0xA1E4380A3B1f749673E270229993eE55F35663b4"的主分片
  // shard 1和shard 2为该账户创建了临时账户

  // 假设current height = 6, 接收到第一个ad_tx
  let height = 6; 
  let ad_tx_shard_id = 1;
  let ad_tx_height = 2;
  let ad_tx_tem_accs = vec![String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4")];
  adt_store.insert(ad_tx_shard_id, ad_tx_height, height, ad_tx_tem_accs.clone()).await;

  // 假设current height = 6, 接收到第二个ad_tx
  let height = 6; 
  let ad_tx_shard_id = 2;
  let ad_tx_height = 2;
  let ad_tx_tem_accs = vec![String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4")];
  adt_store.insert(ad_tx_shard_id, ad_tx_height, height, ad_tx_tem_accs.clone()).await;

  // 假设current height = 6, 接收到第三个ad_tx（模拟收到重复的ad_tx）
  let height = 6; 
  let ad_tx_shard_id = 1;
  let ad_tx_height = 2;
  let ad_tx_tem_accs = vec![String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4")];
  adt_store.insert(ad_tx_shard_id, ad_tx_height, height, ad_tx_tem_accs.clone()).await;



  // 假设current height = 10, 需要对账户进行聚合
  let cur_height = 10;
  let ad_items = adt_store.get_ad(String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4"), cur_height).await;
  
  let expected = vec![
    Item{
      version: 6,
      tem_shard_id: 1,
      creation_height: 2,
    },
    Item {
      version: 6,
      tem_shard_id: 2,
      creation_height: 2
    }
  ];
  let wrong = vec![
    Item{
      version: 6,
      tem_shard_id: 1,
      creation_height: 2,
    },
    Item {
      version: 6,
      tem_shard_id: 2,
      creation_height: 2
    },
    Item {
      version: 6,
      tem_shard_id: 1,
      creation_height: 2
    }    
  ];
  assert_eq!(ad_items, expected);
  assert_ne!(ad_items, wrong);


  // 假设current height = 8,
  let height = 8; 
  let ad_tx_shard_id = 3;
  let ad_tx_height = 3;
  let ad_tx_tem_accs = vec![String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4")];
  adt_store.insert(ad_tx_shard_id, ad_tx_height, height, ad_tx_tem_accs.clone()).await;

  
  // 假设current height = 7（正在执行height 7处的交易）, 需要对账户进行聚合，此时ad_items为空
  let cur_height = 7;
  let ad_items = adt_store.get_ad(String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4"), cur_height).await;
  let expected = vec![]; 
  assert_eq!(ad_items, expected);

  // 假设current height = 20, 需要对账户进行聚合，此时item只有一项
  let cur_height = 20;
  let ad_items = adt_store.get_ad(String::from("0xA1E4380A3B1f749673E270229993eE55F35663b4"), cur_height).await;
  let expected = vec![
    Item{
      version: 8,
      tem_shard_id: 3,
      creation_height: 3,
    }
  ]; 
  assert_eq!(ad_items, expected);


}