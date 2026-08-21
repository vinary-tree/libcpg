export class Ledger {
  constructor() {
    this.entries = [];
  }

  credit(account, amount) {
    if (amount === 0) throw new Error("zero transfer");
    this.entries.push({ account, amount });
  }

  balance(account) {
    return this.entries
      .filter((entry) => entry.account === account)
      .reduce((total, entry) => total + entry.amount, 0);
  }
}

export function transfer(ledger, sender, receiver, amount) {
  if (amount <= 0 || ledger.balance(sender) < amount) return false;
  ledger.credit(sender, -amount);
  ledger.credit(receiver, amount);
  return true;
}
