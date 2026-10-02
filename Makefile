SHELL := cmd.exe
.SHELLFLAGS := /D /C
export PERRY_RUNTIME_DIR := $(CURDIR)/.perry

.PHONY: build build\:production check\:production

build:
	pwsh.exe -NoProfile -File scripts/build.ps1

build\:production: export PERRY_WORKSPACE_ROOT := $(CURDIR)/.perry/source
build\:production: export PERRY_SIZE_OPT := z
build\:production: export PATH := $(USERPROFILE)/.cargo/bin;$(PATH)
build\:production: .perry/source/Cargo.toml
	pwsh.exe -NoProfile -File scripts/build.ps1 -Production

check\:production: build\:production
	node scripts/check-bundle.mjs
	@pwsh.exe -NoProfile -Command "$$before = (Get-FileHash -LiteralPath 'dist/production/drip.exe').Hash; & '$(MAKE)' build:production 'PERRY_WORKSPACE_ROOT=.perry/missing-source' 2>&1 | Tee-Object -Variable rejectedOutput | Out-Null; if ($$LASTEXITCODE -eq 0 -or -not ($$rejectedOutput -match 'Production build requires optimized runtime libraries.')) { throw 'Production fallback guard failed.' }; if ((Get-FileHash -LiteralPath 'dist/production/drip.exe').Hash -ne $$before) { throw 'Failed build replaced the production executable.' }; Write-Output 'Production fallback guard passed.'"

# Match the bundled Perry compiler (0.5.1520).
.perry/source/Cargo.toml:
	git -c advice.detachedHead=false clone --depth 1 --branch v0.5.1520 https://github.com/PerryTS/perry.git .perry/source
