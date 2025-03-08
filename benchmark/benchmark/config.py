# Copyright(C) Facebook, Inc. and its affiliates.
from json import dump, load
from collections import OrderedDict


class ConfigError(Exception):
    pass


# 密钥类
class Key:
    def __init__(self, name, secret):
        self.name = name # 密钥名
        self.secret = secret # 密钥内容

    @classmethod
    def from_file(cls, filename):
        """
            通过文件创建Key实例
        """
        assert isinstance(filename, str)
        with open(filename, 'r') as f:
            data = load(f)
        return cls(data['name'], data['secret'])

# 委员会类，用于管理分布式系统中的多个委员会
class Committees:
  def __init__(self, committees, client_addr, shard_num, shard_size):
    # 初始化委员会类
    self.json = {'shards': committees, 'client': client_addr, 'shard_num': shard_num, 'shard_size': shard_size}

  def shard(self, shard_id):
    # 返回分片数据
    return self.json['shards'][shard_id]

  def client(self):
    # 返回客户端信息
    return self.client()

  def print(self, filename):
    # 将委员会数据写入文件
    assert isinstance(filename, str)
    with open(filename, 'a') as f:
        dump(self.json, f, indent=4, sort_keys=True)

# Committee类，表示一个单独的委员会
class Committee:
    """ The committee looks as follows:
        "authorities: {
            "name": {
                "stake": 1,
                "primary: {
                    "primary_to_primary": x.x.x.x:x,
                    "worker_to_primary": x.x.x.x:x,
                },
                "workers": {
                    "0": {
                        "cross_shard_worker": "x.x.x.x:x",
                        "primary_to_worker": x.x.x.x:x,
                        "worker_to_worker": x.x.x.x:x,
                        "transactions": x.x.x.x:x
                    },
                    ...
                }
            },
            ...
        }
    """

    def __init__(self, addresses, base_port_of_nodes_list, shardid):
        """ The `addresses` field looks as follows:
            {
                "name": ["host", "host", ...],
                ...
            }
        """
        #  addresses = OrderedDict((x, ['127.0.0.1']*(1+workers)) for x in names)
        #  base_port=3000
        # 初始化委员会，地址为字典格式，端口为基准端口列表，shardid为分片ID
        assert isinstance(addresses, OrderedDict)
        assert all(isinstance(x, str) for x in addresses.keys())
        assert all(
            isinstance(x, list) and len(x) > 1 for x in addresses.values()
        )
        assert all(
            isinstance(x, str) for y in addresses.values() for x in y
        )
        assert len({len(x) for x in addresses.values()}) == 1

        self.shardID = shardid  # 0,1

        # self.json = {f'{shardid}': {'authorities': OrderedDict()}}
        # 初始化Json格式
        self.json = {'authorities': OrderedDict()}
        nodeid = 0
        for name, hosts in addresses.items():
            port = base_port_of_nodes_list[nodeid]
            nodeid += 1
            # print(name, hosts, port)
            host = hosts.pop(0)
            
            primary_addr = {
                'primary_to_primary': f'{host}:{port}',
                'worker_to_primary': f'{host}:{port + 1}'
            }
            port += 2

            workers_addr = OrderedDict()
            # 遍历工作节点，分配端口
            for j, host in enumerate(hosts):
                workers_addr[j] = {
                    'primary_to_worker': f'{host}:{port}',
                    'transactions': f'{host}:{port + 1}',
                    'worker_to_worker': f'{host}:{port + 2}',
                    'cross_shard_worker': f'{host}:{port + 3}',
                }
                port += 4

            # 将节点的权限和工作节点信息添加到委员会数据中
            # self.json[f'{shardid}']['authorities'][name]
            self.json['authorities'][name] = {
                'stake': 1,
                'primary': primary_addr,
                'workers': workers_addr
            }

    def authorities(self, faults=0):
        # 返回排除故障节点后的有效委员会成员列表
        assert faults < self.size()
        good_nodes = self.size() - faults
        authorities = list(self.json['authorities'].keys())[:good_nodes]
        return authorities  

    def primary_addresses(self, faults=0):
        # 返回排除故障节点后的有效主节点地址列表
        assert faults < self.size()
        addresses = []
        good_nodes = self.size() - faults
        for authority in list(self.json['authorities'].values())[:good_nodes]:
            addresses += [authority['primary']['primary_to_primary']]
        return addresses

    def workers_addresses(self, faults=0):
        # 返回排除故障节点后的有效工作节点地址列表。
        assert faults < self.size()
        addresses = []
        good_nodes = self.size() - faults
        for authority in list(self.json['authorities'].values())[:good_nodes]: # for each node
            authority_addresses = [] # all worker addrs of a node
            for iD, worker in authority['workers'].items():
                authority_addresses += [(iD, worker['transactions'])]
            addresses.append(authority_addresses)
        return addresses

    def ips(self, name=None):
        # 返回一个节点或所有节点的IP地址列表。
        if name is None:
            names = list(self.json['authorities'].keys())
        else:
            names = [name]
        ips = set()
        for name in names:
            addresses = self.json['authorities'][name]['primary']
            ips.add(self.ip(addresses['primary_to_primary']))
            ips.add(self.ip(addresses['worker_to_primary']))

            for worker in self.json['authorities'][name]['workers'].values():
                ips.add(self.ip(worker['primary_to_worker']))
                ips.add(self.ip(worker['worker_to_worker']))
                ips.add(self.ip(worker['transactions']))

        return list(ips)

    def remove_nodes(self, nodes):
        """ remove the `nodes` last nodes from the committee. """
        # assert nodes < self.size(shardid)
        for _ in range(nodes):
            self.json['authorities'].popitem()

    def size(self):
        """ Returns the number of authorities. """
        return len(self.json['authorities'])
        # return len(self.json['authorities'])

    def good_nodes_size(self, faults=0):
      assert faults < self.size()
      return self.size() - faults

    def workers(self):
        """ Returns the total number of workers (all authorities altogether). """
        return sum(len(x['workers']) for x in self.json['authorities'].values())

    def print(self, filename):
        assert isinstance(filename, str)
        with open(filename, 'a') as f:
            dump(self.json, f, indent=4, sort_keys=True)
        # with open(filename, 'a') as f:


    @staticmethod
    def ip(address):
        assert isinstance(address, str)
        return address.split(':')[0]

# LocalCommittee是Committee类的子类，专门处理本地地址的委员会
class LocalCommittee(Committee):
    def __init__(self, names, port, workers, shardid):
        assert isinstance(names, list)
        assert all(isinstance(x, str) for x in names)
        assert isinstance(port, int)
        assert isinstance(shardid, int)
        assert isinstance(workers, int) and workers > 0
        # committee size: len(names)
        # base_port: port
        base_port_of_nodes = []
        for i in range(len(names)):
          base_port_of_nodes.append(port + i * 6)

        # print(base_port_of_nodes)
        # os._exit(0)

        addresses = OrderedDict((x, ['127.0.0.1'] * (1 + workers)) for x in names)
        super().__init__(addresses, base_port_of_nodes, shardid)

# NodeParameters类，用于管理节点的配置信息
class NodeParameters:
    def __init__(self, json):
        inputs = []
        try:
            inputs += [json['header_size']]
            inputs += [json['max_header_delay']]
            inputs += [json['gc_depth']]
            inputs += [json['sync_retry_delay']]
            inputs += [json['sync_retry_nodes']]
            inputs += [json['batch_size']]
            inputs += [json['max_batch_delay']]
        except KeyError as e:
            raise ConfigError(f'Malformed parameters: missing key {e}')

        if not all(isinstance(x, int) for x in inputs):
            raise ConfigError('Invalid parameters type')

        self.json = json
        self.batch_size = self.tx_size = int(json['batch_size'])

    def print(self, filename):
        assert isinstance(filename, str)
        with open(filename, 'w') as f:
            dump(self.json, f, indent=4, sort_keys=True)

# BenchParameters类，用于管理基准测试的配置参数
class BenchParameters:
    def __init__(self, json):
        try:
            self.faults = int(json['faults'])

            self.cs_faults = int(json['cs_faults'])
            self.sample_interval = int(json['sample_interval'])


            nodes = json['nodes']
            nodes = nodes if isinstance(nodes, list) else [nodes]
            if not nodes or any(x <= 1 for x in nodes):
                raise ConfigError('Missing or invalid number of nodes')
            self.nodes = [int(x) for x in nodes]

            rate = json['rate']
            rate = rate if isinstance(rate, list) else [rate]
            if not rate:
                raise ConfigError('Missing input rate')
            self.rate = [int(x) for x in rate]

            self.workers = int(json['workers'])

            if 'collocate' in json:
                self.collocate = bool(json['collocate'])
            else:
                self.collocate = True

            self.tx_size = int(json['tx_size'])

            self.duration = int(json['duration'])

            self.total_txs = int(json['total_txs'])
            
            # baseline type
            self.state_store_type = int(json['state_store_type'])   
            # self.executor_type = int(json['executor_type'])
            # executor_type
            executor_type = json['executor_type']
            executor_type = executor_type if isinstance(executor_type, list) else [executor_type]
            if not executor_type or any(x < 0 or x > 2 for x in executor_type):
                raise ConfigError('Missing or invalid executor_type')
            self.executor_type = [int(x) for x in executor_type]
            
            self.acc_shard_type = int(json['acc_shard_type'])  
            self.append_type = int(json['append_type'])  

            shard_numbers = json['shard_numbers']
            shard_numbers = shard_numbers if isinstance(shard_numbers, list) else [shard_numbers]
            if not shard_numbers or any(x <= 0 for x in shard_numbers):
                raise ConfigError('Missing or invalid number of shard_numbers')
            self.shard_numbers = [int(x) for x in shard_numbers]
            
            # epoch 
            runs = json['runs']
            runs = runs if isinstance(runs, list) else [runs]
            if not runs or any(x < 0 for x in runs):
                raise ConfigError('Missing or invalid epoch')
            self.runs = [int(x) for x in runs]



        except KeyError as e:
            raise ConfigError(f'Malformed bench parameters: missing key {e}')

        except ValueError:
            raise ConfigError('Invalid parameters type')

        if min(self.nodes) <= self.faults:
            raise ConfigError('There should be more nodes than faults')

# PlotParameters类，用于处理绘图相关的参数
class PlotParameters:
    def __init__(self, json):
        try:
            faults = json['faults']
            faults = faults if isinstance(faults, list) else [faults]
            self.faults = [int(x) for x in faults] if faults else [0]

            nodes = json['nodes']
            nodes = nodes if isinstance(nodes, list) else [nodes]
            if not nodes:
                raise ConfigError('Missing number of nodes')
            self.nodes = [int(x) for x in nodes]

            workers = json['workers']
            workers = workers if isinstance(workers, list) else [workers]
            if not workers:
                raise ConfigError('Missing number of workers')
            self.workers = [int(x) for x in workers]

            if 'collocate' in json:
                self.collocate = bool(json['collocate'])
            else:
                self.collocate = True

            self.tx_size = int(json['tx_size'])

            max_lat = json['max_latency']
            max_lat = max_lat if isinstance(max_lat, list) else [max_lat]
            if not max_lat:
                raise ConfigError('Missing max latency')
            self.max_latency = [int(x) for x in max_lat]

        except KeyError as e:
            raise ConfigError(f'Malformed bench parameters: missing key {e}')

        except ValueError:
            raise ConfigError('Invalid parameters type')

        if len(self.nodes) > 1 and len(self.workers) > 1:
            raise ConfigError(
                'Either the "nodes" or the "workers can be a list (not both)'
            )

    def scalability(self):
        return len(self.workers) > 1
