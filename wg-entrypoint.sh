#!/bin/sh
set -e

# Docker sets net.ipv4.conf.all.src_valid_mark=1 via the compose sysctls directive
# before the container starts. wg-quick tries to set it again at runtime and fails
# with "permission denied". Shadow sysctl with a no-op so wg-quick succeeds — the
# value is already correct.
mkdir -p /tmp/wg-bin
cat > /tmp/wg-bin/sysctl << 'EOF'
#!/bin/sh
exit 0
EOF
chmod +x /tmp/wg-bin/sysctl
export PATH=/tmp/wg-bin:$PATH

if [ ! -f /etc/wireguard/wg0.conf ]; then
    echo "ERROR: /etc/wireguard/wg0.conf not found."
    echo "Mount your WireGuard config: -v /path/to/wg0.conf:/etc/wireguard/wg0.conf:ro"
    exit 1
fi

# The bind mount is read-only so copy to a writable location.
# chmod 600 suppresses wg's "insecure permissions" warning; remove if not needed.
cp /etc/wireguard/wg0.conf /tmp/wg0.conf
chmod 600 /tmp/wg0.conf

echo "Bringing up WireGuard interface..."
wg-quick up /tmp/wg0.conf

echo "Starting tapedeck..."
exec su -s /bin/sh tapedeck -c '/app/tapedeck'
