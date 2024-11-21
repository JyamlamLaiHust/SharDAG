use anyhow::{Context, Result};
use clap::{crate_name, crate_version, App, AppSettings};
use config::{Committees, Import};
use log::info;
use worker::{Account2ShardType, ExecutorType};
use env_logger::Env;
use std::convert::TryFrom;
use std::net::SocketAddr;
use client::{CommonClientMultiTxSenderPerNode, BrokerClientMultiTxSenderPerNode};


const SEND_TX_DURATION_MS: u32 = 6000000; // ms 



#[tokio::main]
async fn main() -> Result<()> {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .about("Benchmark client for Narwhal and Tusk.")
        .args_from_usage("--executor_type=<INT> 'executor_type'")
        .args_from_usage("--acc_shard_type=<INT> 'acc_shard_type'")
        .args_from_usage("--size=<INT> 'The size of each transaction in bytes'")
        .args_from_usage("--rate=<INT> 'The rate (txs/s) at which to send the transactions'")
        .args_from_usage("--nodes=[ADDR]... 'Network addresses that must be reachable before starting the benchmark.'")
        .args_from_usage("--workload=<PATH> 'The path where to load workload'")
        .args_from_usage("--acc2shard=<PATH> 'The path where to load acc2shard map'")
        .args_from_usage("--brokers=<PATH> 'The path where to load brokers'")
        .args_from_usage("--totaltxs=<INT> 'total txs'")
        .args_from_usage("--client_addr=<ADDR> 'client addr used to listen for messages sent by nodes'")
        .args_from_usage("--committee=<FILE> 'The file containing committee information'")
        .args_from_usage("--epoch=<INT> 'The current epoch'")
        .setting(AppSettings::ArgRequiredElseHelp)
        .get_matches();

    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();


    let size = matches
        .value_of("size")
        .unwrap()
        .parse::<usize>()
        .context("The size of transactions must be a non-negative integer")?;
    let rate = matches
        .value_of("rate")
        .unwrap()
        .parse::<u64>()
        .context("The rate of transactions must be a non-negative integer")?;
    let nodes = matches
        .values_of("nodes")
        .unwrap_or_default()
        .into_iter()
        .map(|x| x.parse::<SocketAddr>())
        .collect::<Result<Vec<_>, _>>()
        .context("Invalid socket address format")?;
    let workload_file = matches
        .value_of("workload")
        .unwrap();
    let acc2shard_file = matches
        .value_of("acc2shard")
        .unwrap();
    let brokers_file = matches
        .value_of("brokers")
        .unwrap();      
    let totaltxs = matches
        .value_of("totaltxs")
        .unwrap()
        .parse::<u32>()
        .context("The totaltxs must be a non-negative integer")?;
    let epoch = matches
      .value_of("epoch")
      .unwrap()
      .parse::<usize>()
      .context("The size of transactions must be a non-negative integer")?;

    let client_addr = matches
        .value_of("client_addr")
        .unwrap()
        .parse::<SocketAddr>()
        .unwrap();
    let acc_shard_type = matches
        .value_of("acc_shard_type")
        .unwrap()
        .parse::<usize>()
        .context("acc_shard_type must be a positive integer")?;    
    let executor_type = matches
        .value_of("executor_type")
        .unwrap()
        .parse::<usize>()
        .context("executor_type must be a positive integer")?;
    let acc_shard_type = Account2ShardType::try_from(acc_shard_type).unwrap();           
    let executor_type = ExecutorType::try_from(executor_type).unwrap();
    // load committees
    let committee_file = matches.value_of("committee").unwrap();
    let committees = Committees::import(committee_file).context("Failed to load the committees information")?;
    let shard_num = committees.shard_num;
    let shard_size = committees.shard_size;

    info!("Epoch: {:?}, total txs: {:?}", epoch, totaltxs);
    info!("workload file: {:?}", workload_file);
    info!("acc2shard file: {:?}", acc2shard_file);
    info!("brokers file: {:?}", brokers_file);
    // NOTE: This log entry is used to compute performance.
    info!("Transactions size: {} B", size);
    // NOTE: This log entry is used to compute performance.
    info!("Transactions rate: {} tx/s", rate);
    info!("Shard num: {:?}, Shard size: {:?}", shard_num, shard_size);
    info!("client_addr: {:?}", client_addr);
    info!("wait nodes: {:?}", nodes);

    // create new client: a common client or broker client according to params
    match executor_type {
      ExecutorType::BrokerChain => {
        BrokerClientMultiTxSenderPerNode::spawn(
          shard_num,
          shard_size,
          nodes,
          String::from(workload_file),
          String::from(acc2shard_file),
          String::from(brokers_file),
          acc_shard_type,
          rate,
          totaltxs,
          SEND_TX_DURATION_MS,
          client_addr,
          committees,
          epoch,         
        ).await
      }
      ExecutorType::SharDAG | ExecutorType::Monoxide => { // SharDAG or Monoxide
        CommonClientMultiTxSenderPerNode::spawn(
          shard_num,
          shard_size,
          nodes,
          String::from(workload_file),
          String::from(acc2shard_file),
          acc_shard_type,
          rate,
          totaltxs,
          SEND_TX_DURATION_MS
        ).await
      }
    }
}
