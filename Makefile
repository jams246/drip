SHELL := cmd.exe
.SHELLFLAGS := /D /C
export PERRY_RUNTIME_DIR := $(CURDIR)/.perry

.PHONY: build build\:production check\:production

build:
	@if not exist dist\testing mkdir dist\testing
	".perry\perry.exe" compile src/main.ts -o dist/testing/drip.exe --target windows --debug-symbols --no-auto-optimize

build\:production: export PERRY_WORKSPACE_ROOT := $(CURDIR)/.perry/source
build\:production: export PERRY_SIZE_OPT := z
build\:production: export PATH := $(USERPROFILE)/.cargo/bin;$(PATH)
build\:production: .perry/source/Cargo.toml
	@if not exist dist\production mkdir dist\production
	@pwsh.exe -NoProfile -Command "$$env:PERRY_NO_AUTO_OPTIMIZE = $$null; & '.\.perry\perry.exe' compile src/main.ts -o dist/production/drip.build.exe --target windows --march generic 2>&1 | Tee-Object -Variable buildOutput; if ($$LASTEXITCODE -ne 0) { exit $$LASTEXITCODE }; if ($$buildOutput -match 'workspace source not found|using prebuilt|Skipping auto-optimize') { Remove-Item -LiteralPath 'dist/production/drip.build.exe' -ErrorAction Stop; throw 'Production build requires optimized runtime libraries.' }; Move-Item -LiteralPath 'dist/production/drip.build.exe' -Destination 'dist/production/drip.exe' -Force -ErrorAction Stop"

check\:production: build\:production
	@pwsh.exe -NoProfile -Command "$$before = (Get-FileHash -LiteralPath 'dist/production/drip.exe').Hash; & '$(MAKE)' build:production 'PERRY_WORKSPACE_ROOT=.perry/missing-source' 2>&1 | Tee-Object -Variable rejectedOutput | Out-Null; if ($$LASTEXITCODE -eq 0 -or -not ($$rejectedOutput -match 'Production build requires optimized runtime libraries.')) { throw 'Production fallback guard failed.' }; if ((Get-FileHash -LiteralPath 'dist/production/drip.exe').Hash -ne $$before) { throw 'Failed build replaced the production executable.' }; Write-Output 'Production fallback guard passed.'"

# Match the bundled Perry compiler (0.5.1520).
.perry/source/Cargo.toml:
	git -c advice.detachedHead=false clone --depth 1 --branch v0.5.1520 https://github.com/PerryTS/perry.git .perry/source
