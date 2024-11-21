// Copyright(C) Facebook, Inc. and its affiliates.
use std::collections::{HashMap, VecDeque};
use std::fmt;
use config::Stake;
use crypto::Signature;
use log::debug;
use store::StoreError;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::oneshot;


pub type StoreResult<T> = Result<T, StoreError>;


#[derive(Debug)]
pub enum CSMsgStatus {
  Validating,
  Validated,
  Appended,
  Executed,
}

impl fmt::Display for CSMsgStatus {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "{}", self.to_string())
  }
}


pub enum AppendedType {
  Local, // appended by this node. update_appended is called by csmsg_varification
  Remote, // appended by other node. update_appended is called by processor
}

pub enum CSMsgStoreCommand {
    AddCSMsgSig(String, Signature, oneshot::Sender<StoreResult<(bool, Vec<Signature>)>>), // called by CSMsgVerification
    UpdateAppended(String, AppendedType, oneshot::Sender<StoreResult<bool>>),
    NotifyAppended(String, oneshot::Sender<StoreResult<bool>>), // called by CSMsgVerification
    CanBeExecuted(String, oneshot::Sender<StoreResult<bool>>),
    UpdatedExecuted(String), 
}

#[derive(Clone)]
pub struct CSMsgStore {
    channel: Sender<CSMsgStoreCommand>,
}

impl CSMsgStore {
    pub fn new(vote_threshold: Stake) -> Self {
        // key: f'{[csmsg.source_shard}-{csmsg.cs_msg_id}]'

        let mut csmsg_status_map: HashMap<String, CSMsgStatus> = HashMap::new();
        let mut waiting_csmsg_map: HashMap<String, Vec<Signature>> = HashMap::new();

        let mut obligations = HashMap::<_, VecDeque<oneshot::Sender<_>>>::new();
        let (tx, mut rx) = channel(100);

        tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                match command {
                    // receive a new csmsg from csmsg_verification, process it, return (is_added, thres_sig_list)
                    CSMsgStoreCommand::AddCSMsgSig(id, sig, sender) => {
                      debug!("csmsg_status_map (before addcsmsgsig): {:?}", csmsg_status_map);
                      debug!("waiting_csmsg_map (before addcsmsgsig): {:?}", waiting_csmsg_map);
                      // check the csmsg_status first
                      // if this csmsg is received for the first time, insert an entry (id, CSMsgStatus::Validating)
                      let csmsg_status = csmsg_status_map.entry(id.clone()).or_insert(CSMsgStatus::Validating);
                      debug!("csmsg_status (after addcsmsgsig): {:?}", csmsg_status);
                      debug!("waiting_csmsg_map (after addcsmsgsig): {:?}", waiting_csmsg_map);

                      match *csmsg_status {
                          CSMsgStatus::Validating => { // this csmsg is under validating, add its sig to waiting_csmsg_map
                            // if this csmsg is received for the first time, insert an entry (id, vec![sig]), else, add the new sig to sig_list
                            let thres_sigs = waiting_csmsg_map.entry(id.clone()).or_insert(Vec::new());
                            thres_sigs.push(sig);


                            if thres_sigs.len() == vote_threshold {
                              // collect enough sigs, return thres_sig_list
                              let thres_sigs = waiting_csmsg_map.remove(&id).unwrap();
                              let _ = sender.send(Ok((true, thres_sigs)));  
                              // update csmsg status
                              *csmsg_status = CSMsgStatus::Validated;
                            } else { 
                              let _ = sender.send(Ok((true, Vec::default())));
                            }
                            debug!("csmsg_status (after add): {:?}", csmsg_status);
                            debug!("waiting_csmsg_map (after add): {:?}", waiting_csmsg_map);
                          },
                          _ => { // this csmsg has been validated or appended or executed, just ignore it
                            let _ = sender.send(Ok((false, Vec::default())));
                          }
                      }
                    }      
                    CSMsgStoreCommand::UpdateAppended(id, appended_type, sender) => {
                      match appended_type {
                          AppendedType::Local => {
                            let csmsg_status = csmsg_status_map.get_mut(&id).unwrap();
                            match *csmsg_status {
                              CSMsgStatus::Appended => {
                                let _ = sender.send(Ok(false));
                              },
                              _ => {
                                *csmsg_status = CSMsgStatus::Appended;
                                let _ = sender.send(Ok(true));
                              }                  
                            }
                          },
                          AppendedType::Remote => {
                              debug!("[before] a csmsg is appended by other node: {:?}", csmsg_status_map);
                              // notify appended
                              if let Some(mut senders) = obligations.remove(&id) {
                                while let Some(s) = senders.pop_front() {
                                    let _ = s.send(Ok(true));
                                }
                              }
                              // delete this csmsg from waiting_csmsg_map 
                              waiting_csmsg_map.remove(&id);
                              // override the csmsg_status
                              csmsg_status_map.insert(id, CSMsgStatus::Appended);
                              debug!("[after] a csmsg is appended by other node: {:?}", csmsg_status_map);
                              let _ = sender.send(Ok(true));               
                          }
                      }
                    }
                    // when this func is called, this csmsg has been validated
                    CSMsgStoreCommand::NotifyAppended(id, sender) => {
                        let res = csmsg_status_map.get(&id).unwrap();
                        match res {
                          CSMsgStatus::Appended => {
                            let _ = sender.send(Ok(true));
                          },
                          _ => {// new NotifyAppended
                            obligations
                            .entry(id)
                            .or_insert_with(VecDeque::new)
                            .push_back(sender);
                          }
                        }
                    }
                    CSMsgStoreCommand::CanBeExecuted(id, sender) => {
                      let res = csmsg_status_map.get(&id);
                      match res {
                        None => {
                          let _ = sender.send(Ok(true)); 
                          debug!("this csmsg can be executed");
                        },
                        Some(res) => {
                          match res {
                            CSMsgStatus::Executed => { // this csmsg has been executed
                              let _ = sender.send(Ok(false));
                              debug!("this csmsg has been executed");
                            },
                            _ => {
                              let _ = sender.send(Ok(true));
                              debug!("this csmsg can be executed");
                            }
                          }
                        }                        
                      }
                    }
                    CSMsgStoreCommand::UpdatedExecuted(id) => {
                      csmsg_status_map.insert(id, CSMsgStatus::Executed);
                    }
                }
            }
        });
        Self { channel: tx }
    }


    pub async fn update_appended(&mut self, id: String, appended_type: AppendedType) -> StoreResult<bool> {
      let (sender, receiver) = oneshot::channel();
        if let Err(e) = self.channel.send(CSMsgStoreCommand::UpdateAppended(id, appended_type, sender)).await {
            panic!("Failed to send UpdateAppended command to CSMsgStore: {}", e);
        }
        receiver
            .await
            .expect("Failed to receive reply to AddCSMsgSig command from CSMsgStore")
    }

    pub async fn add_csmsg_sig(&mut self, id: String, sig: Signature) -> StoreResult<(bool, Vec<Signature>)> {
        let (sender, receiver) = oneshot::channel();
        if let Err(e) = self.channel.send(CSMsgStoreCommand::AddCSMsgSig(id, sig, sender)).await {
            panic!("Failed to send AddCSMsgSig command to CSMsgStore: {}", e);
        }
        receiver
            .await
            .expect("Failed to receive reply to AddCSMsgSig command from CSMsgStore")
    }

    pub async fn notify_appended(&mut self, id: String) -> StoreResult<bool> {
        let (sender, receiver) = oneshot::channel();
        if let Err(e) = self
            .channel
            .send(CSMsgStoreCommand::NotifyAppended(id, sender))
            .await
        {
            panic!("Failed to send NotifyAppended command to CSMsgStore: {}", e);
        }
        receiver
            .await
            .expect("Failed to receive reply to NotifyAppended command from CSMsgStore")
    }

    pub async fn can_executed(&mut self, id: String) -> StoreResult<bool> {
      let (sender, receiver) = oneshot::channel();
      if let Err(e) = self
          .channel
          .send(CSMsgStoreCommand::CanBeExecuted(id, sender))
          .await
      {
          panic!("Failed to send CanBeExecuted command to CSMsgStore: {}", e);
      }
      receiver
          .await
          .expect("Failed to receive reply to CanBeExecuted command from CSMsgStore")
    }

    pub async fn update_executed(&mut self, id: String) {
      if let Err(e) = self
          .channel
          .send(CSMsgStoreCommand::UpdatedExecuted(id))
          .await
      {
          panic!("Failed to send UpdatedExecuted command to CSMsgStore: {}", e);
      }
  }
}
