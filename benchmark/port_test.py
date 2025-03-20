import socket
import os

# 定义节点机 IP 列表
nodes = ["192.168.52.141", "192.168.52.142", "192.168.52.143", "192.168.52.144"]
# 定义端口列表
ports = [22, 5009, 5015, 5021, 5027, 5033, 5039, 5045]

for node in nodes:
    # 测试 IP 连通性
    print(f"Testing connectivity to {node}...")
    try:
        # 使用系统的 ping 命令，不同系统的命令参数可能不同，这里以 Linux 为例
        response = os.system(f"ping -c 1 -W 1 {node} > /dev/null 2>&1")
        if response == 0:
            print(f"{node} is reachable.")
            for port in ports:
                print(f"Testing {node}:{port}...")
                try:
                    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                    sock.settimeout(2)
                    result = sock.connect_ex((node, port))
                    if result == 0:
                        print(f"{node}:{port} is reachable")
                    else:
                        print(f"{node}:{port} is unreachable")
                    sock.close()
                except socket.error as e:
                    print(f"Error occurred while testing {node}:{port}: {e}")
        else:
            print(f"{node} is unreachable. Skipping port tests.")
    except Exception as e:
        print(f"Error occurred while pinging {node}: {e}")
    