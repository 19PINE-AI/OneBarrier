# OneBarrier — top-level build.
#
#   make            build everything a first-time user needs
#   make doctor     report which optional dependencies you have
#   make demo       crash an unmodified redis and recover it byte-identically
#   make verify     run the determinism harness over four unmodified servers
#   make test       Rust test suite
#   make clean      remove build outputs

CARGO ?= cargo

.PHONY: all shims engine doctor demo verify test clean help

all: shims engine
	@echo
	@echo "  OneBarrier built. Next:"
	@echo "    make doctor    # what else you may want installed"
	@echo "    make demo      # crash an unmodified redis, recover it byte-identically"
	@echo

## Build the three LD_PRELOAD shims (commodity hardware, no root).
shims:
	@$(MAKE) --no-print-directory -C interpose

## Build the Rust engine and the ob-* tools.
engine:
	$(CARGO) build --release --workspace

doctor:
	@bin/onebarrier doctor

demo: shims engine
	@bin/onebarrier demo

verify: shims engine
	@bash interpose/ob-recover.sh all 3

test:
	$(CARGO) test --workspace --all-targets

clean:
	@$(MAKE) --no-print-directory -C interpose clean
	$(CARGO) clean

help:
	@echo "make          build shims + engine"
	@echo "make doctor   check optional dependencies"
	@echo "make demo     crash an unmodified redis and recover it"
	@echo "make verify   determinism harness over redis, memcached, nginx, node"
	@echo "make test     Rust test suite"
