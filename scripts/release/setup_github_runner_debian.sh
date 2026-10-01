#!/usr/bin/env bash
set -Eeuo pipefail

RUNNER_VERSION="2.335.1"
RUNNER_SHA256="4ef2f25285f0ae4477f1fe1e346db76d2f3ebf03824e2ddd1973a2819bf6c8cf"
RUNNER_ARCHIVE="actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz"
RUNNER_URL="https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/${RUNNER_ARCHIVE}"

DEFAULT_REPO_URL="https://github.com/yedsn/time-genie"
DEFAULT_RUNNER_USER="github-runner"
DEFAULT_RUNNER_DIR="time-genie"
DEFAULT_RUNNER_NAME="time-genie"
DEFAULT_RUNNER_LABELS="self-hosted,linux,x64,gitee-sync"
DEFAULT_DOWNLOAD_PROXY="none"

log() {
  printf '[setup-runner] %s\n' "$*"
}

fail() {
  printf '[setup-runner] Error: %s\n' "$*" >&2
  exit 1
}

prompt_default() {
  local prompt="$1"
  local default_value="$2"
  local value
  read -r -p "${prompt} [${default_value}]: " value
  printf '%s' "${value:-$default_value}"
}

prompt_secret() {
  local prompt="$1"
  local value
  read -r -s -p "${prompt}: " value
  printf '\n' >&2
  printf '%s' "$value"
}

require_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    fail "Please run this script as root, for example: sudo bash $0"
  fi
}

require_debian_like() {
  if [[ ! -r /etc/os-release ]]; then
    fail "/etc/os-release was not found. This script targets Debian-like Linux servers."
  fi
  source /etc/os-release
  case "${ID_LIKE:-$ID}" in
    *debian*|*ubuntu*) ;;
    *) log "Warning: this system is '${PRETTY_NAME:-unknown}', not clearly Debian-like. Continuing." ;;
  esac
}

require_x64() {
  local arch
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) ;;
    *) fail "This script downloads the linux-x64 runner, but this server architecture is '${arch}'." ;;
  esac
}

install_prerequisites() {
  log "Installing required packages: curl, tar, git, python3, ca-certificates."
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y curl tar git python3 ca-certificates
}

run_as_runner_user() {
  local runner_user="$1"
  shift
  if command -v runuser >/dev/null 2>&1; then
    runuser -u "$runner_user" -- "$@"
  else
    su -s /bin/bash "$runner_user" -c "$(printf '%q ' "$@")"
  fi
}

ensure_runner_user() {
  local runner_user="$1"
  if id "$runner_user" >/dev/null 2>&1; then
    log "User '${runner_user}' already exists."
  else
    log "Creating user '${runner_user}'."
    useradd -m -s /bin/bash "$runner_user"
  fi
}

directory_has_runner_files() {
  local install_dir="$1"
  [[ -x "${install_dir}/config.sh" && -x "${install_dir}/svc.sh" ]]
}

prepare_install_dir() {
  local install_dir="$1"
  local runner_user="$2"

  if [[ -d "$install_dir" && -n "$(find "$install_dir" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]]; then
    if directory_has_runner_files "$install_dir" || [[ -f "${install_dir}/${RUNNER_ARCHIVE}" ]]; then
      log "Reusing existing runner directory: ${install_dir}"
    else
      fail "${install_dir} already exists and is not empty, but does not look like a GitHub runner directory. Choose another directory name or clean it manually."
    fi
  else
    log "Preparing install directory: ${install_dir}"
    mkdir -p "$install_dir"
  fi

  chown -R "${runner_user}:${runner_user}" "$install_dir"
}

download_runner() {
  local install_dir="$1"
  local proxy="$2"
  local archive_path="${install_dir}/${RUNNER_ARCHIVE}"
  local curl_args=(--fail --location --retry 3 --output "$archive_path" "$RUNNER_URL")

  if [[ -f "$archive_path" ]]; then
    if printf '%s  %s\n' "$RUNNER_SHA256" "$archive_path" | sha256sum -c - >/dev/null 2>&1; then
      log "Runner archive already exists and passed sha256 check."
      return
    fi
    log "Existing runner archive failed sha256 check. Downloading it again."
  fi

  if [[ -n "$proxy" && "$proxy" != "none" ]]; then
    curl_args=(--proxy "$proxy" "${curl_args[@]}")
    log "Downloading runner through proxy: ${proxy}"
  else
    log "Downloading runner without proxy."
  fi

  curl "${curl_args[@]}"
  printf '%s  %s\n' "$RUNNER_SHA256" "$archive_path" | sha256sum -c -
}

extract_runner_if_needed() {
  local install_dir="$1"
  local runner_user="$2"

  if directory_has_runner_files "$install_dir"; then
    log "Runner files already extracted."
    return
  fi

  log "Extracting runner package."
  tar xzf "${install_dir}/${RUNNER_ARCHIVE}" -C "$install_dir"
  chown -R "${runner_user}:${runner_user}" "$install_dir"
}

run_config_with_optional_proxy() {
  local runner_user="$1"
  local install_dir="$2"
  local proxy="$3"
  local repo_url="$4"
  local runner_token="$5"
  local runner_name="$6"
  local runner_labels="$7"

  local command
  command="cd '$install_dir' && ./config.sh --url '$repo_url' --token '$runner_token' --name '$runner_name' --labels '$runner_labels' --work _work --unattended --replace"

  if [[ -n "$proxy" ]]; then
    log "Configuring runner through proxy: ${proxy}"
    command="export http_proxy='$proxy' https_proxy='$proxy' HTTP_PROXY='$proxy' HTTPS_PROXY='$proxy'; ${command}"
  fi

  run_as_runner_user "$runner_user" bash -lc "$command"
}

main() {
  require_root
  require_debian_like
  require_x64

  local repo_url runner_token runner_name runner_labels runner_user runner_dir_name download_proxy install_dir

  repo_url="$(prompt_default "GitHub repository URL" "$DEFAULT_REPO_URL")"
  runner_token="$(prompt_secret "GitHub runner registration token")"
  [[ -n "$runner_token" ]] || fail "Runner token cannot be empty."

  runner_name="$(prompt_default "Runner name" "$DEFAULT_RUNNER_NAME")"
  runner_labels="$(prompt_default "Runner labels" "$DEFAULT_RUNNER_LABELS")"
  runner_user="$(prompt_default "Linux runner user" "$DEFAULT_RUNNER_USER")"
  runner_dir_name="$(prompt_default "Runner install directory name under /home/${runner_user}" "$DEFAULT_RUNNER_DIR")"
  download_proxy="$(prompt_default "Download proxy, use 'none' to disable" "$DEFAULT_DOWNLOAD_PROXY")"
  [[ "$download_proxy" == "none" ]] && download_proxy=""

  ensure_runner_user "$runner_user"
  install_dir="/home/${runner_user}/${runner_dir_name}"

  install_prerequisites
  prepare_install_dir "$install_dir" "$runner_user"

  download_runner "$install_dir" "$download_proxy"
  extract_runner_if_needed "$install_dir" "$runner_user"

  if [[ -x "${install_dir}/bin/installdependencies.sh" ]]; then
    log "Installing GitHub runner runtime dependencies."
    "${install_dir}/bin/installdependencies.sh"
  fi

  cd "$install_dir"
  if [[ -e "${install_dir}/.runner" ]]; then
    log "Runner is already configured. Skipping config.sh."
  else
    log "Configuring GitHub runner '${runner_name}'."
    run_config_with_optional_proxy "$runner_user" "$install_dir" "$download_proxy" "$repo_url" "$runner_token" "$runner_name" "$runner_labels"
  fi

  log "Installing and starting system service."
  if [[ -f ".service" ]]; then
    log "Runner service is already installed."
  else
    ./svc.sh install "$runner_user"
  fi
  ./svc.sh start
  ./svc.sh status

  log "Done. Check GitHub: ${repo_url}/settings/actions/runners"
  log "Runner labels: ${runner_labels}"
}

main "$@"
