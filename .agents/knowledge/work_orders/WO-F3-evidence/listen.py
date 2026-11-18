import socket, sys, time
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("172.25.6.122", 3483)); s.listen(1); s.settimeout(float(sys.argv[1]))
try:
    c, addr = s.accept()
    c.settimeout(5)
    data = c.recv(256)
    print("connection from", addr, "first bytes:", data[:8], "len", len(data))
    c.close()
except socket.timeout:
    print("no connection within timeout")
