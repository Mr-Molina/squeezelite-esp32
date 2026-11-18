"""TCP relay for the DEVBENCH audio check: this PC (172.25.6.122) forwards the
SlimProto and HTTP stream ports to the LMS at 172.25.8.40, and logs the
SlimProto frames that pass (player STAT events, server strm commands)."""
import os
import socket
import threading
import time

BIND = "172.25.6.122"
LMS = "172.25.8.40"
PORTS = (3483, 9000)
HERE = os.path.dirname(os.path.abspath(__file__))
EVENTS = open(os.path.join(HERE, "slim-events.log"), "a", buffering=1)


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def event(text):
    EVENTS.write(time.strftime("%H:%M:%S") + " " + text + "\n")


def sniff(buf, tag):
    """Log SlimProto frames without touching the forwarded bytes."""
    while True:
        if "dev->lms" in tag:
            if len(buf) < 8:
                return buf
            n = int.from_bytes(buf[4:8], "big")
            if len(buf) < 8 + n:
                return buf
            op, body, buf = buf[:4], buf[8:8 + n], buf[8 + n:]
            if op == b"STAT":
                ev = body[:4].decode("ascii", "replace")
                if ev != "STMt":
                    event("dev STAT " + ev)
            else:
                event("dev %s len=%d" % (op.decode("ascii", "replace"), n))
        else:
            if len(buf) < 2:
                return buf
            n = int.from_bytes(buf[:2], "big")
            if len(buf) < 2 + n:
                return buf
            body, buf = buf[2:2 + n], buf[2 + n:]
            op = body[:4]
            if op == b"strm":
                cmd = chr(body[4])
                if cmd == "s":
                    event("lms strm s fmt=%s hdr=%r" % (chr(body[6]), body[28:100]))
                elif cmd != "t":
                    event("lms strm " + cmd)
            elif op not in (b"audg", b"grfe", b"grfb", b"vfdc", b"visu"):
                event("lms %s len=%d" % (op.decode("ascii", "replace"), n))


def pump(src, dst, tag):
    n = 0
    buf = b""
    try:
        while True:
            d = src.recv(65536)
            if not d:
                break
            dst.sendall(d)
            n += len(d)
            if tag.startswith("3483"):
                try:
                    buf = sniff(buf + d, tag)
                except Exception as e:  # never disturb the relay
                    log("sniff error", e)
                    buf = b""
    except OSError as e:
        log(tag, "error", e)
    finally:
        log(tag, "closed after", n, "bytes")
        for s in (src, dst):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            s.close()


def serve(port):
    ls = socket.socket()
    ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    ls.bind((BIND, port))
    ls.listen(8)
    log("listening", port)
    while True:
        c, addr = ls.accept()
        try:
            u = socket.create_connection((LMS, port), timeout=5)
            u.settimeout(None)
        except OSError as e:
            log(port, "upstream failed", e)
            c.close()
            continue
        c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        u.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        log(port, "accepted", addr)
        if port == 3483:
            event("--- SlimProto connection from %s" % (addr,))
        threading.Thread(target=pump, args=(c, u, "%d dev->lms" % port), daemon=True).start()
        threading.Thread(target=pump, args=(u, c, "%d lms->dev" % port), daemon=True).start()


for p in PORTS:
    threading.Thread(target=serve, args=(p,), daemon=True).start()
while True:
    time.sleep(3600)
