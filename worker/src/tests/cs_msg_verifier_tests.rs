use super::*;
use crate::{common::{committees, keys}, messages::{AD, Address}};
use crypto::{PublicKey, SignatureService};
use tokio::sync::mpsc::channel;




#[tokio::test]
async fn cross_shard_msg_verification_test() {


  let (tx_cross_shard_msg, rx_cross_shard_msg) = channel(1);
  let (tx_batch_maker, rx_batch_maker) = channel(1);

  let id = 0;
  let shardnum = 2;
  let shardid = 1;
  
  // shards
  let all_committees = committees(11_000, shardnum);
  let nodeid = 0;
  let node_nums = 4;

  CSMsgVerification::spawn(
    rx_cross_shard_msg, 
    tx_batch_maker,
    all_committees.clone(),
    2,
    nodeid,
    node_nums,
  );



  let (name, secret) = keys(0).pop().unwrap();
  let mut signature_service = SignatureService::new(secret);


  let mut keys_0 = keys(0);
  let mut signature_services: HashMap<PublicKey, SignatureService> = HashMap::new();

  while let Some((name, secret)) = keys_0.pop() {
    let mut signature_service = SignatureService::new(secret);
    signature_services.insert(name, signature_service);
  }


  // 等待接收消息

  // let expected: GeneralTransaction;
  // let output = rx_batch_maker.recv().await.unwrap();
  //assert_eq!(output, expected);  
  
  // 模拟CrossShardReceiverHandler向CrossShardMsgVerification发送GeneralTransaction::AD
  // 四个节点发送消息
  let tem_accs: Vec<Address> = vec![String::from("0xc1c03b8fa379f6ca8a11833f9c2e0775a9715907")];


  for (name, signature_service) in signature_services.iter_mut(){

    let ad = AD::new(
      0, 2, 1, tem_accs.clone(), 
      &name, signature_service
    ).await;
    let msg = GeneralTransaction::AD(ad);
    
    tx_cross_shard_msg.send(msg).await.unwrap(); 
  
  }







}