.PHONY: all
all: clean reinstall_ledgers reinstall

.PHONY: clean
.SILENT: clean
clean:
	rm -rf .dfx
	cargo clean
	dfx build --ic

# ----
# export NET=ic
# export PRINCIPAL=$(dfx identity get-principal)
# ----
.PHONY: ledgers
.SILENT: ledgers
ledgers:
	dfx deploy --network="$(NET)" icrc1_ledger_syron_btc --argument '(variant { Init = record { token_symbol = "BTC"; token_name = "BTC Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "$(PRINCIPAL)" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 2_100_000_000_000_000; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'\
	&& dfx deploy --network="$(NET)" icrc1_ledger_syron_susd --argument '(variant { Init = record { token_symbol = "SU$D"; token_name = "Syron US Dollar"; decimals = opt 8; minting_account = record { owner = principal "$(PRINCIPAL)" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 10_000_000_000_000_000_000; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'

.PHONY: reinstall_ledgers
.SILENT: reinstall_ledgers
reinstall_ledgers:
	dfx canister --ic install --mode=reinstall \
	icrc1_ledger_syron_btc --argument '(variant { Init = record { token_symbol = "BTC"; token_name = "BTC Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'\
	&& dfx canister --ic install --mode=reinstall \
	icrc1_ledger_syron_susd --argument '(variant { Init = record { token_symbol = "SU$D"; token_name = "Syron U$ Dollar"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'

# ----
# export BTC_LEDGER=$(dfx canister id --ic icrc1_ledger_syron_btc) SUSD_LEDGER=$(dfx canister id --ic icrc1_ledger_syron_susd) SSI=tb1pjnr5curwpqcxhyxjfcqmya3ms48s7ca7erd6uxwx6dp3svunq2wsq4me33
# ----

.PHONY: deploy
.SILENT: deploy
deploy:
	dfx deploy --network="$(NET)" basic_bitcoin_tyron --argument '(variant { testnet }, variant { Init = record { mode = variant { GeneralAvailability }; btc_network = variant { Testnet }; ledger_id = principal "$(BTC_LEDGER)"; susd_id = principal "$(SUSD_LEDGER)"; xrc_id = principal "uf6dk-hyaaa-aaaaq-qaaaq-cai"; ecdsa_key_name = "test_key_1"; min_confirmations = opt 1; retrieve_btc_min_amount = 600; max_time_in_queue_nanos = 600_000_000_000 } })'

.PHONY: box
.SILENT: box
box:
# dfx canister call basic_bitcoin_tyron get_box_address "(record { owner = opt principal \"$(PRINCIPAL)\"; subaccount = null;})"
# dfx canister call basic_bitcoin_tyron get_box_address "(record { owner = opt principal \"$(PRINCIPAL)\"})"
# dfx canister call basic_bitcoin_tyron get_box_address "(record { subaccount=null;})"

# Add SSI
# dfx canister call basic_bitcoin_tyron get_box_address "(record { owner = opt principal \"$(PRINCIPAL)\"; subaccount = null; ssi=\"$(SSI)\";})"
	dfx canister --ic call basic_bitcoin_tyron get_box_address "(record { ssi=\"$(SSI)\";})"

.PHONY: susd
.SILENT: susd
susd:
	dfx canister --ic call basic_bitcoin_tyron get_susd "(record { ssi=\"$(SSI)\";})"

.PHONY: reinstall
.SILENT: reinstall
reinstall:
	dfx canister --ic install --mode=reinstall basic_bitcoin_tyron --argument '(variant { testnet }, variant { Init = record { mode = variant { GeneralAvailability }; btc_network = variant { Testnet }; ledger_id = principal "evswi-eiaaa-aaaap-ab3rq-cai"; susd_id = principal "eavhf-faaaa-aaaap-ab3sa-cai"; xrc_id = principal "uf6dk-hyaaa-aaaaq-qaaaq-cai"; ecdsa_key_name = "test_key_1"; min_confirmations = opt 1; retrieve_btc_min_amount = 0; max_time_in_queue_nanos = 600_000_000_000 } })'
# max_time_in_queue_nanos = 600_000_000_000 is 10 minutes (600 billion nanoseconds)
# dfx canister --network="$(NET)" install --all --mode=upgrade basic_bitcoin_tyron

.PHONY: btc_minter
.SILENT: btc_minter
btc_minter:
	dfx canister --ic call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \"ehubr-iyaaa-aaaap-ab3sq-cai\" })"

.PHONY: cy
.SILENT: cy
cy:
	dfx canister --ic call gyjkd-saaaa-aaaap-abxra-cai wallet_balance

#dfx wallet --network=ic balance

.PHONY: generate
.SILENT: generate
generate:
	dfx generate basic_bitcoin_tyron

.PHONY: subaccounts
.SILENT: subaccounts
subaccounts:
	dfx canister --ic call basic_bitcoin_tyron get_subaccount "( 1, \"$(SSI)\" )"
	dfx canister --ic call basic_bitcoin_tyron get_subaccount "( 2, \"$(SSI)\" )"

.PHONY: bal_susd
.SILENT: bal_susd
bal_susd:
	dfx canister --ic call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BOX)\" })"
	dfx canister --ic call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BAL)\" })"

.PHONY: bal_susd_minter
.SILENT: bal_susd_minter
bal_susd_minter:
	dfx canister --ic call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \"ehubr-iyaaa-aaaap-ab3sq-cai\" })"

.PHONY: bal_btc
.SILENT: bal_btc
bal_btc:
	dfx canister --ic call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BOX)\" })"
	dfx canister --ic call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BAL)\" })"

#subaccount = opt blob \"\1f\bc\3b\f8\22\a0\c5\21\5d\55\48\a2\1c\e5\4c\d4\a3\41\4d\7d\3a\c1\bb\00\52\0d\8e\29\70\ba\c4\9d\" })"

.PHONY: bal_btc_minter
.SILENT: bal_btc_minter
bal_btc_minter:
	dfx canister --ic call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \"ehubr-iyaaa-aaaap-ab3sq-cai\" })"

.PHONY: minter_info
.SILENT: minter_info
minter_info:
	dfx canister --ic call basic_bitcoin_tyron get_minter_info

.PHONY: minter
.SILENT: minter
minter:
	dfx canister --ic call basic_bitcoin_tyron get_p2wpkh_address

.PHONY: topup
.SILENT: topup
topup:
	dfx canister deposit-cycles --ic 1_000_000_000_000 ehubr-iyaaa-aaaap-ab3sq-cai
