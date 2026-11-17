#!/bin/bash
set -e

# Local test runner for robot tests against local dev services.
# Expects:
#   - Backend running on localhost:8080 (cargo run or dev server)
#
# Usage:
#   ./run-tests-local.sh                     # Run all tests
#   ./run-tests-local.sh tests/test_be.robot # Run specific test file

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Local service URLs (overridable via environment)
LOCAL_BE_URL="${LOCAL_BE_URL:-http://localhost:8080}"

# Report output directory
REPORT_DIR="${SCRIPT_DIR}/reports"
rm -rf "${REPORT_DIR}"
mkdir -p "${REPORT_DIR}"

# Default test path (can be overridden via first argument)
TEST_PATH="${1:-${SCRIPT_DIR}/tests}"

# Resolve relative paths
if [[ ! "${TEST_PATH}" = /* ]]; then
    TEST_PATH="${SCRIPT_DIR}/${TEST_PATH}"
fi

echo "=============================================="
echo " Data-Engine Robot Tests - Local Dev Runner"
echo "=============================================="
echo "Backend URL:  ${LOCAL_BE_URL}"
echo "Test Path:    ${TEST_PATH}"
echo "Report Dir:   ${REPORT_DIR}"
echo "=============================================="

# Pre-flight checks
echo ""
echo "Running pre-flight checks..."

if curl -sf "${LOCAL_BE_URL}/api/v1/health" > /dev/null 2>&1; then
    echo "  ✓ Backend is responding at ${LOCAL_BE_URL}"
else
    echo "  ✗ Backend is NOT responding at ${LOCAL_BE_URL}"
    echo "    Start it with: make preflight && cargo run --manifest-path de-be-container/Cargo.toml -- -c de-be-container/config/default.yaml start"
    exit 1
fi

echo ""
echo "Starting robot tests..."
echo ""

robot \
    --variable BE_BASE_URL:${LOCAL_BE_URL} \
    --variable BE_POD_IP: \
    --exclude k8s_only \
    --loglevel DEBUG \
    -d "${REPORT_DIR}" \
    "${TEST_PATH}"
