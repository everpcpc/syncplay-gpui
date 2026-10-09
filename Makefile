.PHONY: bump-major bump-minor bump-patch format run test lint build

# Get current version from the app crate manifest
CURRENT_VERSION := $(shell grep -m1 '^version' crates/app/Cargo.toml | cut -d'"' -f2)

# Parse version components
MAJOR := $(shell echo $(CURRENT_VERSION) | cut -d. -f1)
MINOR := $(shell echo $(CURRENT_VERSION) | cut -d. -f2)
PATCH := $(shell echo $(CURRENT_VERSION) | cut -d. -f3)

run:
	@echo "Starting Syncplay in development mode..."
	@cargo run -p syncplay 2>&1 | tee debug.log

build:
	@echo "Building Syncplay for production..."
	@cargo build --release -p syncplay

format:
	@echo "Formatting Rust code..."
	@cargo fmt --all
	@echo "All code formatted successfully"

test:
	@echo "Running Rust tests..."
	@cargo test --workspace
	@echo "All tests completed"

lint:
	@echo "Linting Rust code..."
	@cargo clippy --workspace --all-targets
	@cargo fmt --all -- --check
	@echo "Lint checks completed"

bump-major:
	@echo "Bumping major version from $(CURRENT_VERSION)"
	$(eval NEW_VERSION := $(shell echo $$(($(MAJOR) + 1)).0.0))
	@$(MAKE) update-version NEW_VERSION=$(NEW_VERSION)

bump-minor:
	@echo "Bumping minor version from $(CURRENT_VERSION)"
	$(eval NEW_VERSION := $(MAJOR).$(shell echo $$(($(MINOR) + 1))).0)
	@$(MAKE) update-version NEW_VERSION=$(NEW_VERSION)

bump-patch:
	@echo "Bumping patch version from $(CURRENT_VERSION)"
	$(eval NEW_VERSION := $(MAJOR).$(MINOR).$(shell echo $$(($(PATCH) + 1))))
	@$(MAKE) update-version NEW_VERSION=$(NEW_VERSION)

update-version:
	@echo "Updating version to $(NEW_VERSION)"
	@perl -0pi -e 's/^version = ".*?"$$/version = "$(NEW_VERSION)"/m' crates/app/Cargo.toml
	@cargo metadata --format-version 1 > /dev/null
	@git add crates/app/Cargo.toml Cargo.lock
	@git commit -m "chore: bump version to $(NEW_VERSION)"
	@echo "Version bumped to $(NEW_VERSION) and committed"
	@echo "Run 'git push origin main' to trigger the release workflow"
