# SharDAG

The artifact is provided as a git repository and contains a simplified version of our SharDAG prototype implementation, build instructions, and deployment scripts.


## Scope

Our artifact can be used to validate our claim that SharDAG outperforms state-of-the-art non-sharding/non-adaptive sharding DAG-based blockchains, i.e., a non-sharding scheme based on BullShark and two sharding schemes using state-of-the-art sharding protocols, BrokerChain and Monoxide.



## Contents

The artifact includes the source code of the SharDAG prototype, which is implemented based on the source code of BullShark, the state-of-the-art DAG-based BFT consensus protocol. Specifically, SharDAG uses BullShark as the intra-shard consensus and executes transactions in a shard serially. The core protocols are written in Rust, but all benchmarking scripts are written in Python and run with [Fabric](http://www.fabfile.org/).

The artifact contains the following key sub-directories:

* `./worker`, which includes the source code of SharDAG implementation.
* `./client`, which includes the source code of benchmark clients, i.e., a common client for SharDAG and Monoxide-HS baseline and a broker client for BrokerChain-HS baseline.
* `./node`, which includes the code to boot the node and benchmark client and the test code for the state storage model 
* `./benchmark`, which includes all the experiment scripts used to help deploy systems and run experiments.

Besides, the `README.md` file provides deployment and usage instructions for users.



## Requirements

The experiments in our paper are conducted on a testbed consisting of 17 virtual machines on Alibaba Cloud. The artifact has the following hardware/software requirements:

- **Hardware Requirement**: The hardware platform contains 17 machines, each of which is configured by {CPU: Intel Xeon Platinum 8369B @3.5GHz, Cores: 16, DRAM: 32GB, Disk: 100GB SSD}.

- **Software Requirement:** Operating system: {Ubuntu 20.04 LTS}, Runtime language: {Rust v1.70, Python 3.10}, Clang 10, tmux 3.0a. All software dependencies are included in the `Cargo.toml` file and `benchmarks/requirements.txt` file.

  

## Deploy

Step 1: install Rust, Clang (required by rocksdb) and [tmux](https://linuxize.com/post/getting-started-with-tmux/#installing-tmux) (used to run all nodes and clients in the background)

    $ apt-get -y install build-essential
    $ apt-get -y install cmake
    $ curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    $ source $HOME/.cargo/env
    $ rustup default stable
    $ apt-get install -y clang
    $ apt-get install -y tmux
Step 2: install repo


Clone the repo and install the python dependencies:

```
$ git clone https://github.com/cfll19/SharDAG.git
$ cd SharDAG/benchmark
$ pip install -r requirements.txt
```
Step 3: run a local benchmark

Run a local benchmark on your local machine using fabric to check that the environment is configured successfully.

The default configuration is a testbed of 8 nodes (2 shards * 4 nodes per shard)

```
$ fab local
```
This command may take a long time the first time you run it (compiling rust code in `release` mode may be slow) and you can customize a number of benchmark parameters in `fabfile.py`. When the benchmark terminates, it writes a summary of the execution to `results/local-xxxx.txt`.

Now that SharDAG has been successfully deployed, you can run local benchmarks to test SharDAG locally.



## Run local benchmark

Step 1: Parametrize the benchmark

Locate the task called `local` in the file `fabfile.py`. 

```
@task
def local(ctx):
    ...
```

The task specifies two types of parameters, the *benchmark parameters* and the *nodes parameters*. The *benchmark parameters* look as follows:

```
bench_params = {
  'shard_numbers': [2],
  'nodes': [4],
  'faults': 0,
  'cs_faults': 0,
  'workers': 1, # # of workers per node
  'runs': [1], # times, epoch
  'rate': [200], # transaction sending rate per node
  'duration': 40, # running duration
  'total_txs': 1000000, # 1000000
  'tx_size': 512, # B
  'acc_shard_type': 0, # 0: Hash, 1: Graph
  'executor_type': 0, # 0: SharDAG, 1: Monoxide, 2: Broker
  'state_store_type': 1, # 0: TStore, 1: MStore. Monoxide and Broker use MStore. 
  'append_type': 0, # 0: Dual-mode, 1: Serial
}
```

They specify the number of shards (`shard_numbers`), the number of nodes per shard (`nodes`) to deploy, the number of cross-shard faults per shard (`cs_faults`), the number of times to repeat each benchmark (`runs`), the input rate per node (tx/s) at which the clients submits transactions to the system (`rate`, the total input rate of the whole system is rate * shard_number * nodes), the duration of the benchmark in seconds (`duration`), the total number of transactions injected into the system (`total_txs`), the size of each transaction in bytes (`tx_size`), the account assignment strategy (`acc_shard_type`), the cross-shard transaction processing mechanism (`executor_type`), the state storage model (`state_store_type`), and the cross-shard message appending mechanism (`append_type`). The `faults` parameter donotes that the number of fault nodes in intra-shard consensus. Since our focus is Byzantine resilient cross-shard message verification, we set `faults` to 0. All nodes will be booted and participate honestly in the intra-shard consensus. However, when the parameters `cs_faults` is set to `f > 0`, the first `f` nodes will behave honestly within the shard but maliciously across shards , i.e., do not forward, verify, and package cross-shard messages.

Modify the two types of parameters according to your Settings.

Step 2: Run the benchmark

Once you specified both `bench_params` and `node_params` as desired, run:

```
$ fab local
```

This command first recompiles your code in release mode (and with the benchmark feature flag activated), thus ensuring you always benchmark the latest version of your code.   This may take a long time the first time you run it.   It then generates the configuration files and keys for each node, and runs the benchmarks with the specified parameters. It finally parses the log and saves an execution summary to a result file located in the `results` subfolder, e.g, `lan-Hash-SharDAG-MStore-8-10-0-3-200-300-10000000-500000B.txt`.



## Run lan benchmark

Step 1: Configure the remote host information

Modify the remote host information in the `benchmark/lan_remote.py` file, including

* `self.hosts`: the ip addresses of the remote nodes you're using to run blockchain nodes
* `self.user` and `self.connect`: the username and password for the remote connection
* `self.nodes_per_host`: the maximum number of nodes running on each remote host. The nodes are distributed across hosts as evenly as possible.

Step 2: Configure the environment for the remote hosts and deploy codes

Run  `remoteinstall`  using Fabric: 

```
$ fab remoteinstall  
```

The above command will install the required environments for the remote host to run the SharDAG node, including Rust, Clang, tmux, git, and clone the latest code from the github repository.

Step 3: Run a benchmark

After setting up the testbed, running a benchmark on WAN testbed is similar to running it locally. Locate the task `remote` in `fabfile.py`:

```
@task
def remote(ctx):
    ...
```

The benchmark parameters are similar to local benchmarks. 

Once you specified both `bench_params` and `node_params` as desired, run:

```
$ fab remote
```

This command first updates all machines with the latest commit of the GitHub repo and branch specified in the file `benchmark/lan_remote.py`;  this ensures that benchmarks are always run with the latest version of the code.  It then generates and uploads the configuration files to each machine, runs the benchmarks with the specified parameters, and downloads the logs.  It finally parses the logs and prints the results into a folder called results (which is automatically created if it doesn't already exists).  You can run fab remote multiple times without fearing to override previous results.



Step 3: Plot the results

Once you have collected enough result data, the following instructions can be used to plot the overall tps and latency graphs.

Locate the task called `plot` in the file `fabfile.py`. You can set the `input_rates` parameter to specify the input rate.

```
@task
def plot(ctx):
    try:
        input_rates = [100, 200]
        plot = Plot()
        plot.draw_tps_latency(input_rates)
        ...
```

Then run `plot` using Fabric: 

```
$ fab plot
```
This command creates the tps and latency graphs in a folder called `data` (which is automatically created if it doesn't already exists). 



