

#[cfg(test)]
mod tests {
    use ic_ckbtc_minter_tyron::updates::update_balance::UpdateBalanceError;
    use mockall::{automock, predicate::*};

    // @dev Structs and Enums
    pub struct CollateralizedAccount {
        pub exchange_rate: u64,
        pub collateral_ratio: u64,
        pub btc_1: u64,
        pub susd_1: u64,
    }

    pub struct ExchangeRate {
        pub rate: u64,
    }

    #[derive(Debug, PartialEq)]
    pub enum SyronLedger {
        BTC,
        SUSD,
    }

    // @dev Traits for mocking
    #[automock]
    pub trait ExchangeRateProvider {
        fn get_exchange_rate(&self) -> Result<ExchangeRate, UpdateBalanceError>;
    }

    #[automock]
    pub trait BalanceProvider {
        fn balance_of(&self, ledger: SyronLedger, ssi: &str, id: u64) -> Result<u64, UpdateBalanceError>;
    }

    // @dev Function to test
    fn get_collateralized_account(exchange_rate_provider: &dyn ExchangeRateProvider, balance_provider: &dyn BalanceProvider, ssi: &str, dummy: bool) -> Result<CollateralizedAccount, UpdateBalanceError> {
        let xr = exchange_rate_provider.get_exchange_rate()?;
        let btc_1 = balance_provider.balance_of(SyronLedger::BTC, ssi, 1).unwrap_or(0);
        let susd_1 = balance_provider.balance_of(SyronLedger::SUSD, ssi, 1).unwrap_or(0);
        
        let exchange_rate: u64 = if dummy {
            if btc_1 != 0 {
                (1.15 * susd_1 as f64 / btc_1 as f64) as u64
            } else {
                xr.rate / 1_000_000_000 / 137 * 100
            }
        } else {
            xr.rate / 1_000_000_000
        };

        let collateral_ratio = if btc_1 == 0 || susd_1 == 0 {
            15000 // 150%
        } else {
            ((btc_1 as f64 * exchange_rate as f64 / susd_1 as f64) * 10000.0) as u64
        };

        Ok(CollateralizedAccount {
            exchange_rate,
            collateral_ratio,
            btc_1,
            susd_1
        })
    }

    #[test]
    fn test_get_collateralized_account() {
        let ssi = "tb1p4w59p7nxggc56lg79v7cwh4c8emtudjrtetgasfy5j3q9r4ug9zsuwhykc";

        let mut mock_exchange_rate_provider = MockExchangeRateProvider::new();
        let mut mock_balance_provider = MockBalanceProvider::new();

        mock_exchange_rate_provider.expect_get_exchange_rate()
            .returning(|| Ok(ExchangeRate { rate: 100_000_000_000_000 }));

        mock_balance_provider.expect_balance_of()
            .with(eq(SyronLedger::BTC), eq(ssi), eq(1))
            .returning(|_, _, _| Ok(0));
        
        mock_balance_provider.expect_balance_of()
            .with(eq(SyronLedger::SUSD), eq(ssi), eq(1))
            .returning(|_, _, _| Ok(0));

        let result = get_collateralized_account(&mock_exchange_rate_provider, &mock_balance_provider, ssi, false);

        match result {
            Ok(account) => {
                assert_eq!(account.exchange_rate, 100000);
                assert_eq!(account.collateral_ratio, 15000);
                assert_eq!(account.btc_1, 0);
                assert_eq!(account.susd_1, 0);
            }
            Err(_) => panic!("get_collateralized_account failed"),
        }
    }
}
