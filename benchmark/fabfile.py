# Copyright(C) Facebook, Inc. and its affiliates.
from fabric import task # type: ignore
from benchmark.local import LocalBench
from benchmark.utils import Print, BenchError
from benchmark.remote import RemoteBench
from benchmark.plot import Plot

@task
def local(ctx, debug=False):
    ''' Run benchmarks on localhost '''
    bench_params = {
        'shard_numbers': [4, 6, 8, 10, 12, 14, 16],
        'nodes': [4],
        'faults': 0,
        'cs_faults': 0,
        'sample_interval': 1,

        'workers': 1, # # of workers per node
        'runs': [1], # times, epoch

        'rate': [100, 200], # transaction sending rate per node
        'duration': 40, # running duration
        'total_txs': 1000000, # 1000000
        'tx_size': 512, # B 事务大小为512B

        'acc_shard_type': 0, # 0: Hash, 1: Graph
        'executor_type': 0, # 0: SharDAG, 1: Monoxide, 2: Broker
        'state_store_type': 0, # 0: TStore, 1: MStore. Monoxide and Broker use MStore.
        'append_type': 0, # 0: Dual-mode, 1: Serial
    }
    node_params = {
        'header_size': 50,  # bytes
        'max_header_delay': 1000,  # ms
        'gc_depth': 50,  # rounds
        'sync_retry_delay': 10_000,  # ms
        'sync_retry_nodes': 3,  # number of nodes
        'batch_size': 500_000,  # bytes 最大块大小为500MB
        'max_batch_delay': 200  # ms
    }
    try:
        ret = LocalBench(bench_params, node_params).run(debug)
    except BenchError as e:
        Print.error(e)


@task
def remote(ctx, debug=False, remote_recompile=True):
    ''' Run benchmarks in remotehost '''
    bench_params = {
        'shard_numbers': [4],
        # 'shard_numbers': [14, 12, 10, 8, 6, 4],
        'nodes': [10],
        'faults': 0,
        'cs_faults': 3,
        'workers': 1,
        # 'runs': [0], # epoch i
        'runs': [1], # repeat `runs` times for each [shard_number, nodes]

        'rate': [100, 200], # total transaction sending rate
        'duration':300, # running duration
        'total_txs': 10000000, 
        'tx_size': 512, # B

        'acc_shard_type': 0, # 0: Hash, 1: Graph
        'executor_type': [0, 1, 2], # 0: SharDAG, 1: Monoxide, 2: Broker
        'state_store_type': 1, # 0: TStore, 1: MStore
        'append_type': 1, # 0: Dual-mode, 1: Serial
    }
    node_params = {
        'header_size': 50,  # bytes
        'max_header_delay': 1_000,  # ms 200ms
        'gc_depth': 50,  # rounds
        'sync_retry_delay': 10_000,  # ms
        'sync_retry_nodes': 3,  # number of nodes
        'batch_size': 500_000,  # bytes
        'max_batch_delay': 200  # ms
    }
    try:
        RemoteBench(ctx).run(bench_params, node_params, debug, remote_recompile)
    except BenchError as e:
        Print.error(e)


@task
def remoteinstall(ctx):
    ''' for remoteinstall '''
    try:
        remotebench = RemoteBench(ctx)
        # remotebench.install()
        # remotebench.install_repo()
        remotebench.unload_input_files()
    except BenchError as e:
        Print.error(e)


@task
def plot(ctx):
    ''' plot the overall tps and latency graphs'''
    try:
        input_rates = [100, 200]
        plot = Plot()
        plot.draw_tps_latency(input_rates)
    except BenchError as e:
        Print.error(e)