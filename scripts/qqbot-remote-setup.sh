#!/usr/bin/env bash
set -euo pipefail

# Bootstrap a fresh AliCloud ECS instance for the qqbot service.
# Usage: qqbot-remote-setup.sh <service-user> <install-dir>

SERVICE_USER="${1:-qqbot}"
INSTALL_DIR="${2:-/opt/qqbot}"

echo "Installing Docker..."
if command -v dnf &>/dev/null; then
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
usermod -aG docker "${SERVICE_USER}" || true

# Allow the service user to manage the qqbot unit without a password.
if [[ "$(id -un)" == "root" ]]; then
    SUDOERS_FILE="/etc/sudoers.d/99-qqbot"
    if ! grep -q "${SERVICE_USER} ALL=(ALL) NOPASSWD: /usr/bin/systemctl" "${SUDOERS_FILE}" 2>/dev/null; then
        echo "${SERVICE_USER} ALL=(ALL) NOPASSWD: /usr/bin/systemctl * qqbot" > "${SUDOERS_FILE}"
        chmod 440 "${SUDOERS_FILE}"
    fi
fi

echo "Creating install directory ${INSTALL_DIR}..."
mkdir -p "${INSTALL_DIR}/bin"
mkdir -p "${INSTALL_DIR}/data"
chown -R "${SERVICE_USER}:${SERVICE_USER}" "${INSTALL_DIR}/data"
chmod 755 "${INSTALL_DIR}"

echo "Pulling SnowLuma image..."
docker pull motricseven7/snowluma:latest || true

echo "Remote setup complete."
