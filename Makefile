# PolecatWorks Data-Engine Makefile

.PHONY: help build test test-be seed-validation-suite preflight clean \
        de-be-docker de-fe-docker helm-lint helm-package \
        garden-up garden-test garden-down squash-gh-pages

HELM_CHART := data-engine
HELM_DIR := charts/$(HELM_CHART)
ROBOT_REPORT_DIR := integration-tests/reports

help:
	@echo "PolecatWorks Data-Engine Targets:"
	@echo "  make build                  - Build de-be-container Rust backend"
	@echo "  make test                   - Run all backend unit and integration tests"
	@echo "  make preflight              - Validate configuration and CLI preflight"
	@echo "  make seed-validation-suite  - Generate deterministic Parquet chaos & drift suite"
	@echo "  make de-be-docker           - Build production Docker image for backend"
	@echo "  make de-fe-docker           - Build production Docker image for frontend"
	@echo "  make helm-lint              - Lint the charts/data-engine Helm chart"
	@echo "  make garden-up              - Deploy full stack to Kubernetes via Garden"
	@echo "  make garden-test            - Run in-cluster Robot Framework integration tests via Garden"
	@echo "  make garden-down            - Teardown Garden Kubernetes environment"
	@echo "  make squash-gh-pages        - Squash gh-pages commit history older than 8 days"

build:
	cargo build --manifest-path de-be-container/Cargo.toml

test: test-be

test-be:
	cargo test --manifest-path de-be-container/Cargo.toml -- --test-threads=1

preflight:
	cargo run --manifest-path de-be-container/Cargo.toml -- -c de-be-container/config/default.yaml preflight

seed-validation-suite:
	cargo run --manifest-path de-be-container/Cargo.toml -- -c de-be-container/config/default.yaml seed-validation-suite --output-dir /tmp/de-validation-suite

de-be-docker:
	cd de-be-container && docker build -t data-engine-be:latest .

de-fe-docker:
	cd de-fe-container && docker build -t data-engine-fe:latest .

helm-lint:
	helm lint $(HELM_DIR)

helm-package:
	helm package $(HELM_DIR) -d charts

garden-up:
	@echo "Logging Helm into GHCR and running Garden deploy..."
	@echo "$${GHCR_TOKEN}" | helm registry login ghcr.io -u "$${GHCR_USER:-bengreen}" --password-stdin 2>/dev/null || true
	garden deploy

garden-test: garden-up
	@echo "Running Garden tests..."
	garden test
	@echo "Copying test reports to $(ROBOT_REPORT_DIR)..."
	@mkdir -p $(ROBOT_REPORT_DIR)
	@NS="data-engine-$${USER:-local}"; \
	kubectl cp $$NS/robot-test-runner:/tmp/reports $(ROBOT_REPORT_DIR) 2>/dev/null || true

garden-down:
	@echo "Tearing down Garden environment..."
	garden cleanup env

squash-gh-pages:
	@set -e; \
	if [ "$$CONFIRM_SQUASH" != "true" ]; then \
		echo "ERROR: You must set CONFIRM_SQUASH=true to execute this action."; \
		exit 1; \
	fi; \
	echo "Checking out gh-pages..."; \
	git checkout gh-pages; \
	git pull origin gh-pages || true; \
	echo "Squashing history older than 8 days on gh-pages..."; \
	HASH=$$(git log --before="8 days ago" --format="%h" -1); \
	if [ -z "$$HASH" ]; then \
		echo "No commits older than 8 days found. Falling back to the oldest commit..."; \
		HASH=$$(git log --reverse --format="%h" | head -n 1); \
	fi; \
	if [ -z "$$HASH" ]; then \
		echo "Could not find any commits. Exiting."; \
		exit 1; \
	fi; \
	echo "Found commit: $$HASH"; \
	echo "Creating temporary orphan branch at $$HASH..."; \
	git branch -D temp-branch || true; \
	git checkout --orphan temp-branch $$HASH; \
	echo "Creating base commit..."; \
	PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit --no-verify -m "Squashed history older than 8 days"; \
	echo "Rebasing recent commits onto the new base..."; \
	git rebase --onto temp-branch $$HASH gh-pages; \
	echo "Cleaning up temp branch..."; \
	git branch -D temp-branch; \
	echo "Removing content older than 8 days..."; \
	printf '%s\n' \
		'import os, time, shutil' \
		'now = time.time()' \
		'for folder in ["pr", "merge-group"]:' \
		'    if os.path.exists(folder):' \
		'        for entry in os.listdir(folder):' \
		'            path = os.path.join(folder, entry)' \
		'            if os.path.isdir(path):' \
		'                ts_file = os.path.join(path, ".timestamp")' \
		'                if os.path.exists(ts_file):' \
		'                    try:' \
		'                        with open(ts_file, "r") as f:' \
		'                            ts = float(f.read().strip())' \
		'                            if now - ts > 8 * 24 * 3600:' \
		'                                print(f"Pruning old folder: {path}")' \
		'                                shutil.rmtree(path)' \
		'                    except Exception as e:' \
		'                        print(f"Error checking {path}: {e}")' \
		> prune.py; \
	python3 prune.py; \
	rm prune.py; \
	git add -u; \
	if ! git diff --quiet || ! git diff --cached --quiet; then \
		echo "Committing pruned content..."; \
		PRE_COMMIT_ALLOW_NO_CONFIG=1 git commit --no-verify -m "Prune reports older than 8 days"; \
	fi; \
	echo "Force pushing to origin..."; \
	git push -f origin gh-pages; \
	echo "Done."

run-ablation-benchmark:
	cargo run --manifest-path de-be-container/Cargo.toml -- -c de-be-container/config/default.yaml run-ablation-benchmark --output-dir /tmp/de-ablation-suite
clean:
	cargo clean --manifest-path de-be-container/Cargo.toml
