package service

import "errors"

type Entry struct {
	Account string
	Amount  int64
}

type Ledger struct {
	Entries []Entry
}

func (ledger *Ledger) Credit(account string, amount int64) error {
	if amount == 0 {
		return errors.New("zero transfer")
	}
	ledger.Entries = append(ledger.Entries, Entry{Account: account, Amount: amount})
	return nil
}

func (ledger *Ledger) Balance(account string) int64 {
	var total int64
	for _, entry := range ledger.Entries {
		if entry.Account == account {
			total += entry.Amount
		}
	}
	return total
}
