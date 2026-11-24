import argparse
import socket


def parse_args():
    parser = argparse.ArgumentParser(description="Listen for a single TCP connection.")
    parser.add_argument("timeout", type=float, help="Accept timeout in seconds")
    parser.add_argument("--host", default="172.25.6.122", help="Bind host/IP")
    parser.add_argument("--port", type=int, default=3483, help="Bind port")
    return parser.parse_args()


def main():
    args = parse_args()
    with socket.socket() as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((args.host, args.port))
        s.listen(1)
        s.settimeout(args.timeout)
        try:
            c, addr = s.accept()
            with c:
                c.settimeout(5)
                data = c.recv(256)
                print("connection from", addr, "first bytes:", data[:8], "len", len(data))
        except socket.timeout:
            print("no connection within timeout")


if __name__ == "__main__":
    main()
