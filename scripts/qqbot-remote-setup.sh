#!/usr/bin/env bash
set -euo pipefail

# Bootstrap a fresh AliCloud ECS instance for the qqbot service.
# Usage: qqbot-remote-setup.sh <service-user> <install-dir>

SERVICE_USER="${1:-qqbot}"
INSTALL_DIR="${2:-/opt/qqbot}"

echo "Installing Docker..."
if command -v apt-get &>/dev/null; then
    apt-get update || true
    apt-get install -y docker.io || true
elif command -v dnf &>/dev/null; then
    dnf install -y docker || true
elif command -v yum &>/dev/null; then
    yum install -y docker || true
else
    echo "No supported package manager found; assuming Docker is pre-installed."
fi

if command -v systemctl &>/dev/null; then
    systemctl enable --now docker || true
fi

echo "Creating service user ${SERVICE_USER}..."
if ! id -u "${SERVICE_USER}" &>/dev/null; then
    useradd --system --create-home --shell /bin/bash "${SERVICE_USER}"
fi
if getent group docker >/dev/null; then
    usermod -aG docker "${SERVICE_USER}" || true
fi

# Allow the service user to manage the qqbot unit and install files without a password.
if [[ "$(id -un)" == "root" ]]; then
    SUDOERS_FILE="/etc/sudoers.d/99-qqbot"
    cat > "${SUDOERS_FILE}" <<EOF
${SERVICE_USER} ALL=(ALL) NOPASSWD: /usr/bin/systemctl *
${SERVICE_USER} ALL=(ALL) NOPASSWD: /usr/bin/mv -f /tmp/qqbot.service /etc/systemd/system/qqbot.service
EOF
    chmod 440 "${SUDOERS_FILE}"
fi

echo "Creating install directory ${INSTALL_DIR}..."
mkdir -p "${INSTALL_DIR}/bin"
mkdir -p "${INSTALL_DIR}/data"
chown -R "${SERVICE_USER}:${SERVICE_USER}" "${INSTALL_DIR}"
chmod 755 "${INSTALL_DIR}"

echo "Configuring Docker Hub mirror..."
if [[ ! -f /etc/docker/daemon.json ]]; then
    mkdir -p /etc/docker
    cat > /etc/docker/daemon.json <<EOF
{"registry-mirrors": ["https://docker.1panel.live"]}
EOF
    if command -v systemctl &>/dev/null; then
        systemctl restart docker || true
    fi
fi

echo "Pulling SnowLuma image..."
docker pull motricseven7/snowluma:latest || true

echo "Remote setup complete."
