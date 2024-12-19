
use serde::{Deserialize, Serialize};
use mpt::{ MPTStore, MPTStoreTrait, MMPTStore};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::{File, self};
use hex::FromHex;
use worker::{Address, StateStoreType, TStore, MStore, StateStore, INIT_BALANCE, AccToShardItem};
use csv::DeserializeRecordsIter;
use tokio::time::Instant;
use worker::Account;


#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct NewRes {
  pub method: String,
  pub epoch: u64,
  pub total_accs: u64,
  pub active_accs: u64,
  pub out_accs: u64,
  pub load_dur_ms: u128,
  // res
  pub total_dur: u128, 
  pub mig_data_size_b: usize,
}

// TODO clear dormant acc
#[tokio::main]
async fn main(){
  println!("Run main() in h_store_main.rs");

  // 初始化账户文件路径
  let init_acc_file = "/root/SharDAG-WorkSpace/inputv2/acc2shard-e9-s2.csv";

  // 结果保存路径
  let res_path = "../test-state-store/res-migration.csv";
  let mut wtr = csv::Writer::from_path(res_path).unwrap();

  // 定义要测试的方法集
  let methods = vec![String::from("MStore"), String::from("TStore")];
  let test_method: usize = 1; // 0: MStore; 1: TStore
  let state_store_type = StateStoreType::try_from(test_method).unwrap();  

  // test epoch：0, 6, 12, 18, 24, 29
  // // 定义测试的 epoch 参数，键为 epoch 值，值为 (总账户数量, 活跃账户数量, 迁移账户数量)
  let mut params: HashMap<u64, (u64, u64, u64)> = HashMap::new(); // Epoch -> (TotalAccNum, ActiveAccNum, OutAccNum)
  // params.insert(0, (90223, 90223, 78995.5 as u64));
  // params.insert(3, (290607.625 as u64, 89523.625 as u64, 76111.5 as u64));
  // params.insert(6, (454961.75 as u64, 90164.75 as u64, 70991.5 as u64));
  // params.insert(9, (607454.125 as u64, 91654.75 as u64, 77126.375 as u64));
  // params.insert(12, (752526.125 as u64, 92826 as u64, 81382.625 as u64));
  // params.insert(15, (896661.25 as u64, 91568.625 as u64, 82935 as u64));

  // params.insert(18, (1044320.75 as u64, 91423.125 as u64, 81779.375 as u64));
  // params.insert(21, (1193288.75 as u64, 95906.125 as u64, 85782.125 as u64));
  // params.insert(24, (1347386.5 as u64, 99994.875 as u64, 89975.75 as u64));
  params.insert(27, (1508491.75 as u64, 95226.875 as u64, 78604.875 as u64));
  // params.insert(29, (1604722.125 as u64, 102043.75 as u64, 87547.875 as u64));

  let times = 1; // 每个 epoch 运行的次数
  let shard_id = 0; // 当前分片 ID
  let target_shard_id = 0; // 目标分片 ID
  let full_t_path = "test_db_full_t"; // 状态存储路径

  // 遍历每个 epoch 的参数
  for (epoch, (total_acc_nums, active_acc_nums, out_acc_nums)) in &params{
    let mut times = times;
    while times > 0 { 
      times -= 1;

      // 清理测试目录
      let _ = fs::remove_dir_all(full_t_path);
      let _ = fs::remove_dir_all("test_db_act_t");
      let _ = fs::remove_dir_all("test_db_act_new");

      // 初始化状态存储
      let (mut state_store, out_act_accs, out_dor_accs, act_accs, dor_accs, load_dur_ms) = _initial_store_test(full_t_path, state_store_type.clone(), init_acc_file, *total_acc_nums, *active_acc_nums, *out_acc_nums).await;

      // 获取根哈希值
      let (res1, res2) = state_store.root().await;
      let act_root_hash = res1.unwrap();
      let full_root_hash = res2.unwrap();

      // 执行迁移并记录迁移数据大小和耗时
      let (mig_data_size_b, total_dur) = state_store.test_migration(out_act_accs, out_dor_accs, *epoch, shard_id, target_shard_id, act_root_hash.clone(), full_root_hash.clone(), act_accs, dor_accs).await;

      // 验证前以后根哈希值是否一致
      let (res1, res2) = state_store.root().await; 
      let act_root_hash_after = res1.unwrap();
      let full_root_hash_after = res2.unwrap();

      println!("act_root_hash after migraton: {:?}", act_root_hash_after == act_root_hash);
      println!("full_root_hash after migraton: {:?}", full_root_hash_after == full_root_hash);

      // 将测试结果写入 CSV 文件
      let new_res = NewRes{
        method: methods[test_method].clone(),
        epoch: *epoch,
        total_accs: *total_acc_nums,
        active_accs: *active_acc_nums,
        out_accs: *out_acc_nums,
        load_dur_ms,
        total_dur,
        mig_data_size_b
      };

      wtr.serialize(new_res).unwrap();      
      wtr.flush().unwrap();
    }
  } 
}

// 初始化状态存储函数
pub async fn _initial_store_test(
  full_t_path: &str,
  state_store_type: StateStoreType,
  acc2shard_file: &str, 
  total_acc_nums: u64, active_acc_nums: u64, out_acc_nums: u64
) -> (
  Box<dyn StateStore + Send>, 
  Vec<Address>, Vec<Address>, 
  Vec<Address>, Vec<Address>, 
  u128
) {
  let mut active_accs_list :Vec<Address> = Vec::default(); // 活跃账户列表
  let mut out_accs_list: Vec<Address> = Vec::default(); // 即将迁移的用户列表
  let mut act_accs :Vec<Address> = Vec::default(); // 活跃账户（部分）
  let dor_accs: Vec<Address> = Vec::default();  // 不活跃账户

  // value
  // 定义账户的初始值
  let acc = Account { nonce: 0, balance: INIT_BALANCE};
  let serialized = bincode::serialize(&acc).expect("Failed to serialize account");

  // 初始化 MPT 存储
  let mut full_t = MPTStore::new(full_t_path);

  let mut index = 0; 
  let mut reader = csv::Reader::from_path(acc2shard_file).unwrap();
  let mut state_iter: DeserializeRecordsIter<File, AccToShardItem> = reader.deserialize().into_iter();
  println!("begin load state: {} accounts, {} active accs, {} out accs", total_acc_nums, active_acc_nums, out_acc_nums);
  let before_load = Instant::now();

  while let Some(Ok(acc_shard)) = state_iter.next(){
    let addr = Vec::from_hex(&acc_shard.account[2..]).unwrap();

    if index < out_acc_nums { // outgoing act acc
      out_accs_list.push(addr.clone());
    } else if index < active_acc_nums { // left act acc
      act_accs.push(addr.clone());
    }

    if index < active_acc_nums {
      active_accs_list.push(addr.clone());   
    }

    full_t.insert(addr, serialized.clone()).await.unwrap();

    index += 1;
    if index == total_acc_nums {
      break;
    }
  }
  let dur = before_load.elapsed().as_millis();
  println!("loading state takes {} ms", dur);
  println!("active_accs: {}, out_accs: {}", active_accs_list.len(), out_accs_list.len());

  // create state store
  let store: Box<dyn StateStore + Send>;
  match state_store_type {
    StateStoreType::MStore => {
      println!("Initialize MStore");
      store =  Box::new(MStore { shard_id: 0, full_t, insert_dur: Vec::default(), get_dur: Vec::default()});
    }
    StateStoreType::TStore => {
      println!("Initialize TStore");
      let mut act_t = MMPTStore::new();
      // initialize act_t
      for addr in active_accs_list {
        let _ = act_t.insert(addr, serialized.clone()).await.unwrap();
      }
      store = Box::new(TStore { shard_id: 0, act_t, full_t, insert_dur: Vec::default(), get_act_dur: Vec::default(), get_full_dur: Vec::default()});
    }
  }

  (store, out_accs_list, Vec::default(), act_accs, dor_accs, dur)
}