"""TCP/UDP port reference for the "port" keyword (Sevak one-shot plugin).

"port 443" says what the port is normally used for, "port ssh" or "port
postgres" finds a service by name. The table is built in, so it works offline
and never scans or contacts anything. Standard library only.
"""
import json
import sys

# port, name, protocol, what it is
PORTS = [
    (20, "FTP data", "TCP", "File Transfer Protocol, data channel"),
    (21, "FTP", "TCP", "File Transfer Protocol, control channel"),
    (22, "SSH", "TCP", "Secure Shell, also SFTP and SCP"),
    (23, "Telnet", "TCP", "Unencrypted remote login; avoid"),
    (25, "SMTP", "TCP", "Mail transfer between servers"),
    (53, "DNS", "TCP/UDP", "Domain name system"),
    (67, "DHCP server", "UDP", "Hands out IP addresses (68 is the client)"),
    (80, "HTTP", "TCP", "Web traffic, unencrypted"),
    (110, "POP3", "TCP", "Mail download"),
    (119, "NNTP", "TCP", "Usenet news"),
    (123, "NTP", "UDP", "Network time"),
    (143, "IMAP", "TCP", "Mail access"),
    (161, "SNMP", "UDP", "Network device monitoring"),
    (389, "LDAP", "TCP", "Directory services"),
    (443, "HTTPS", "TCP", "Web traffic over TLS (QUIC / HTTP/3 uses UDP 443)"),
    (445, "SMB", "TCP", "Windows file sharing"),
    (465, "SMTPS", "TCP", "Mail submission over TLS"),
    (514, "Syslog", "UDP", "System log messages"),
    (587, "SMTP submission", "TCP", "Mail submission from clients (STARTTLS)"),
    (636, "LDAPS", "TCP", "LDAP over TLS"),
    (853, "DNS over TLS", "TCP", "Encrypted DNS"),
    (873, "rsync", "TCP", "rsync daemon"),
    (993, "IMAPS", "TCP", "IMAP over TLS"),
    (995, "POP3S", "TCP", "POP3 over TLS"),
    (1080, "SOCKS", "TCP", "SOCKS proxy"),
    (1194, "OpenVPN", "UDP", "OpenVPN default"),
    (1433, "SQL Server", "TCP", "Microsoft SQL Server"),
    (1521, "Oracle DB", "TCP", "Oracle database listener"),
    (1883, "MQTT", "TCP", "Message broker for IoT"),
    (2049, "NFS", "TCP/UDP", "Network File System"),
    (2181, "ZooKeeper", "TCP", "Apache ZooKeeper client port"),
    (2375, "Docker", "TCP", "Docker daemon, unencrypted (2376 is TLS)"),
    (2379, "etcd", "TCP", "etcd client port"),
    (3000, "Dev server", "TCP", "Node, Rails, Grafana and many dev servers"),
    (3306, "MySQL", "TCP", "MySQL and MariaDB"),
    (3389, "RDP", "TCP", "Windows Remote Desktop"),
    (4000, "Dev server", "TCP", "Phoenix, Jekyll and other dev servers"),
    (4173, "Vite preview", "TCP", "vite preview"),
    (4222, "NATS", "TCP", "NATS messaging"),
    (5000, "Dev server", "TCP", "Flask default; also Docker registry"),
    (5173, "Vite", "TCP", "Vite dev server"),
    (5432, "PostgreSQL", "TCP", "PostgreSQL"),
    (5601, "Kibana", "TCP", "Kibana web interface"),
    (5672, "RabbitMQ", "TCP", "AMQP (management UI is 15672)"),
    (5900, "VNC", "TCP", "Virtual Network Computing"),
    (6379, "Redis", "TCP", "Redis"),
    (6443, "Kubernetes API", "TCP", "Kubernetes API server"),
    (8000, "Dev server", "TCP", "Django, Python http.server"),
    (8080, "HTTP alt", "TCP", "Alternate web port, proxies, Tomcat, Jenkins"),
    (8443, "HTTPS alt", "TCP", "Alternate TLS web port"),
    (8888, "Jupyter", "TCP", "Jupyter notebooks"),
    (9000, "Dev server", "TCP", "PHP-FPM, SonarQube, MinIO and others"),
    (9090, "Prometheus", "TCP", "Prometheus server"),
    (9092, "Kafka", "TCP", "Apache Kafka broker"),
    (9200, "Elasticsearch", "TCP", "Elasticsearch HTTP API"),
    (11211, "Memcached", "TCP/UDP", "Memcached"),
    (27017, "MongoDB", "TCP", "MongoDB"),
]


def row(entry):
    port, name, proto, what = entry
    return {
        "key": f"{port}-{name}",
        "title": f"{port}  {name} ({proto})",
        "subtitle": what + ". Enter to copy the port number",
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": str(port)},
    }


def results(query):
    text = query.strip().lower()
    if not text:
        return [row(e) for e in PORTS if e[0] in (22, 53, 80, 443, 3000, 3306, 5173, 5432, 6379, 8080, 27017)]
    if text.isdigit():
        number = int(text)
        hits = [e for e in PORTS if e[0] == number]
        if hits:
            return [row(e) for e in hits]
        if number > 65535:
            return [{"key": "range", "title": "Ports go from 0 to 65535", "subtitle": "A port number is 16 bits",
                     "icon": {"kind": "builtin", "name": "copy"}, "action": {"type": "copy_text", "text": "65535"}}]
        kind = ("well-known (0-1023, needs admin rights to listen on)" if number < 1024
                else "registered (1024-49151)" if number < 49152 else "dynamic / ephemeral (49152-65535)")
        return [{"key": "range", "title": f"{number} is not in this list", "subtitle": f"It is a {kind} port",
                 "icon": {"kind": "builtin", "name": "copy"}, "action": {"type": "copy_text", "text": text}}]
    words = text.split()
    hits = [e for e in PORTS if all(w in f"{e[1]} {e[3]}".lower() for w in words)]
    if not hits:
        return [{"key": "none", "title": "No port with that name in the list", "subtitle": "Try a number (5432) or a name (postgres, ssh, redis)",
                 "icon": {"kind": "builtin", "name": "copy"}, "action": {"type": "copy_text", "text": "443"}}]
    return [row(e) for e in hits[:20]]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
