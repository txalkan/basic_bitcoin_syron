# export NET=ic OR =local
# export BTC_LEDGER=$(dfx canister --ic id icrc1_ledger_syron_btc) SUSD_LEDGER=$(dfx canister --ic id icrc1_ledger_syron_susd)
# export ECDSA_KEY=test_key_1 OR =dfx_test_key
# export PRINCIPAL=$(dfx identity get-principal)
# export LIQ=tb1pduxfg234ckmc3mq5znzhhtgukg79c3emc9d42twafhdcgk5rgxcqxwpu35
# export SSI=tb1p4w59p7nxggc56lg79v7cwh4c8emtudjrtetgasfy5j3q9r4ug9zsuwhykc
# export TXID=dc437f680283e69efa18a30384ec84c1a1deaa7ff96e39c364c5eacd5445bc9d
# export TX_ID=dac2ccdb26c09907fe10da83048e8b8cd7e3320cfdd4587388e36424abe6ba8f

# .PHONY: all
# all: clear_ledgers clear clean re_ledgers reinstall

.PHONY: all
all: clear clean reinstall

.PHONY: clean
clean:
	rm -rf .dfx
	cargo clean
	dfx build --ic 
# --network="$(NET)"

.PHONY: re_ledgers
re_ledgers:
	dfx canister --ic install --mode=reinstall \
	icrc1_ledger_syron_btc --argument '(variant { Init = record { token_symbol = "BTC"; token_name = "BTC Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'\
	&& dfx canister --ic install --mode=reinstall \
	icrc1_ledger_syron_susd --argument '(variant { Init = record { token_symbol = "SUSD"; token_name = "SUSD Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'

.PHONY: reinstall
#.SILENT: reinstall
reinstall:
	dfx canister --ic install --mode=reinstall basic_bitcoin_tyron --argument '(variant { testnet }, variant { Init = record { mode = variant { GeneralAvailability }; btc_network = variant { Testnet }; ledger_id = principal "$(BTC_LEDGER)"; susd_id = principal "$(SUSD_LEDGER)"; xrc_id = principal "uf6dk-hyaaa-aaaaq-qaaaq-cai"; ecdsa_key_name = "$(ECDSA_KEY)"; min_confirmations = opt 1; retrieve_btc_min_amount = 10; max_time_in_queue_nanos = 600_000_000_000 } })'
# max_time_in_queue_nanos = 600_000_000_000 is 10 minutes (600 billion nanoseconds)
# dfx canister --network="$(NET)" install --all --mode=upgrade basic_bitcoin_tyron

.PHONY: subaccounts
.SILENT: subaccounts
subaccounts:
	dfx canister --ic call basic_bitcoin_tyron get_subaccount "( 1, \"$(SSI)\" )"
	dfx canister --ic call basic_bitcoin_tyron get_subaccount "( 2, \"$(SSI)\" )"
	dfx canister --ic call basic_bitcoin_tyron get_subaccount "( 3, \"$(SSI)\" )"

.PHONY: bal_susd
.SILENT: bal_susd
bal_susd:
	dfx canister --ic call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(SDB)\" })"
	dfx canister --ic call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BAL)\" })"
	dfx canister --ic call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BRC20)\" })"

.PHONY: add_bal
add_bal:
	dfx canister --ic call basic_bitcoin_tyron update_ssi_balance "(record { ssi=\"$(SSI)\"; op=variant { getsyron }})"

# # # # # # # 

# clear was needed for dfx 0.19
.PHONY: clear
.SILENT: clear
clear:
	dfx canister call --ic aaaaa-aa stored_chunks '(record { canister_id = principal "ehubr-iyaaa-aaaap-ab3sq-cai" })'
	dfx canister call --ic aaaaa-aa clear_chunk_store '(record { canister_id = principal "ehubr-iyaaa-aaaap-ab3sq-cai" })'

.PHONY: clear_ledgers
clear_ledgers:
	dfx canister call --ic aaaaa-aa stored_chunks '(record { canister_id = principal "evswi-eiaaa-aaaap-ab3rq-cai" })'
	dfx canister call --ic aaaaa-aa clear_chunk_store '(record { canister_id = principal "evswi-eiaaa-aaaap-ab3rq-cai" })'
	dfx canister call --ic aaaaa-aa stored_chunks '(record { canister_id = principal "eavhf-faaaa-aaaap-ab3sa-cai" })'
	dfx canister call --ic aaaaa-aa clear_chunk_store '(record { canister_id = principal "eavhf-faaaa-aaaap-ab3sa-cai" })'

.PHONY: local
local:
	dfx start --clean

# dfx canister create basic_bitcoin_tyron
.PHONY: ledgers
ledgers:
	dfx deploy --network="$(NET)" icrc1_ledger_syron_btc --argument '(variant { Init = record { token_symbol = "BTC"; token_name = "BTC Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'\
	&& dfx deploy --network="$(NET)" icrc1_ledger_syron_susd --argument '(variant { Init = record { token_symbol = "SUSD"; token_name = "SUSD Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'

.PHONY: reinstall_ledgers
.SILENT: reinstall_ledgers
reinstall_ledgers:
	dfx canister --network="$(NET)" install --mode=reinstall \
	icrc1_ledger_syron_btc --argument '(variant { Init = record { token_symbol = "BTC"; token_name = "BTC Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'\
	&& dfx canister --network="$(NET)" install --mode=reinstall \
	icrc1_ledger_syron_susd --argument '(variant { Init = record { token_symbol = "SUSD"; token_name = "SUSD Syron Ledger"; decimals = opt 8; minting_account = record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai" }; transfer_fee = 0; metadata = vec {}; feature_flags = opt record { icrc2 = true }; initial_balances = vec { record { record { owner = principal "ehubr-iyaaa-aaaap-ab3sq-cai"; }; 0; }; }; archive_options = record { num_blocks_to_archive = 1000; trigger_threshold = 2000; controller_id = principal "$(PRINCIPAL)"; cycles_for_archive_creation = opt 10000000000000 }}})'

.PHONY: syron
syron:
	dfx deploy --network="$(NET)" basic_bitcoin_tyron --argument '(variant { regtest }, variant { Init = record { mode = variant { GeneralAvailability }; btc_network = variant { Regtest }; ledger_id = principal "$(BTC_LEDGER)"; susd_id = principal "$(SUSD_LEDGER)"; xrc_id = principal "uf6dk-hyaaa-aaaaq-qaaaq-cai"; ecdsa_key_name = "$(ECDSA_KEY)"; min_confirmations = opt 1; retrieve_btc_min_amount = 600; max_time_in_queue_nanos = 600_000_000_000 } })'

.PHONY: sdb
.SILENT: sdb
sdb:
# dfx canister call basic_bitcoin_tyron get_box_address "(record { owner = opt principal \"$(PRINCIPAL)\"; subaccount = null; ssi=\"$(SSI)\";})"
	dfx canister --network="$(NET)" call basic_bitcoin_tyron get_box_address "(record { ssi=\"$(SSI)\";})"

.PHONY: redeem_bal
redeem_bal:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron update_ssi_balance "(record { ssi=\"$(SSI)\"; op=variant { redeembitcoin }})"

.PHONY: btc_minter
.SILENT: btc_minter
btc_minter:
	dfx canister --network="$(NET)" call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \"ehubr-iyaaa-aaaap-ab3sq-cai\" })"

.PHONY: cy
.SILENT: cy
cy:
	dfx canister --network="$(NET)" call gyjkd-saaaa-aaaap-abxra-cai wallet_balance

# dfx wallet --network=ic balance

.PHONY: generate
.SILENT: generate
generate:
	dfx generate basic_bitcoin_tyron

.PHONY: susd_minter
.SILENT: susd_minter
susd_minter:
	dfx canister --network="$(NET)" call icrc1_ledger_syron_susd icrc1_balance_of "(record { owner = principal \"ehubr-iyaaa-aaaap-ab3sq-cai\" })"

.PHONY: bal_btc
.SILENT: bal_btc
bal_btc:
	dfx canister --ic call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(SDB)\" })"
	dfx canister --ic call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \
	\"ehubr-iyaaa-aaaap-ab3sq-cai\"; \
	subaccount = opt blob \"$(BAL)\" })"

# subaccount = opt blob \"\1f\bc\3b\f8\22\a0\c5\21\5d\55\48\a2\1c\e5\4c\d4\a3\41\4d\7d\3a\c1\bb\00\52\0d\8e\29\70\ba\c4\9d\" })"

.PHONY: bal_btc_minter
.SILENT: bal_btc_minter
bal_btc_minter:
	dfx canister --network="$(NET)" call icrc1_ledger_syron_btc icrc1_balance_of "(record { owner = principal \"ehubr-iyaaa-aaaap-ab3sq-cai\" })"

.PHONY: info
info:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron get_minter_info

.PHONY: p2wpkh
p2wpkh:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron get_p2wpkh_address

.PHONY: topup
.SILENT: topup
topup:
	dfx canister deposit-cycles --ic 300_000_000_000 ehubr-iyaaa-aaaap-ab3sq-cai

.PHONY: logs
logs:
	dfx canister --network="$(NET)" logs basic_bitcoin_tyron

.PHONY: faucet
.SILENT: faucet
faucet:
	@read -p "Enter coupon: " coupon; \
	dfx wallet --ic redeem-faucet-coupon $$coupon

.PHONY: test
test:
	cargo test -- --nocapture RUST_BACKTRACE=1

.PHONY: withdrawal
withdrawal:
	@read -p "Enter user wallet: " ssi; \
	read -p "Enter transaction ID: " txid; \
	read -p "Enter cycles cost: " cost; \
	dfx canister --network="$(NET)" call basic_bitcoin_tyron withdraw_susd "(record { ssi=\"$$ssi\"; op=variant { getsyron }}, \"$$txid\", $$cost)"

# --wallet $(shell dfx identity get-wallet --ic) --with-cycles $$cost

.PHONY: withdraw
withdraw:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron withdraw_susd "(record { ssi=\"$(SSI)\"; op=variant { getsyron }}, \"$(TXID)\", 72_000_000, 1)"

.PHONY: inscription
inscription:
	dfx canister --ic call basic_bitcoin_tyron get_inscription "(\"$(TXID)\", 72_000_000, 1)"

.PHONY: susd
susd:
	@read -p "Use IC (y/n)? " ic;\
    if [ "$$ic" = "y" ]; then \
        dfx canister --ic call basic_bitcoin_tyron get_susd "( record { ssi=\"$(SSI)\"}, \"$(TXID)\" )"; \
    else \
        dfx canister call basic_bitcoin_tyron get_susd "( record { ssi=\"$(SSI)\"}, \"$(TXID)\" )"; \
    fi

.PHONY: utxos
utxos:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron test

.PHONY: percentiles
percentiles:
	dfx canister --ic call basic_bitcoin_tyron get_current_fee_percentiles

.PHONY: in_bal
in_bal:
	dfx canister --ic call basic_bitcoin_tyron get_indexed_balance "(\"$(SSI)\")"

.PHONY: sbal
sbal:
	@read -p "Enter address: " addr; \
	dfx canister --ic call basic_bitcoin_tyron get_indexed_balance "( \"$$addr\" )"

.PHONY: redeem
redeem:
	dfx canister --ic call basic_bitcoin_tyron redeem_btc "( record { ssi=\"$(SSI)\"; op=variant { redeembitcoin }} )"; \

.PHONY: account
account:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron get_account "( \"$(SSI)\", true )"

.PHONY: sdb_susd
sdb_susd:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron susd_balance_of "( \"$(SSI)\", 1 )"

.PHONY: sdb_btc
sdb_btc:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron sbtc_balance_of "( \"$(SSI)\", 1 )"

.PHONY: liq
liq:
	dfx canister --network="$(NET)" call basic_bitcoin_tyron liquidate "( record { ssi=\"$(SSI)\"; op=variant { redeembitcoin }}, \"$(LIQ)\", \"$(TX_ID)\" )"
