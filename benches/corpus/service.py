class Ledger:
    def __init__(self):
        self.entries = []

    def credit(self, account, amount):
        if amount <= 0:
            raise ValueError("amount must be positive")
        self.entries.append((account, amount))

    def balance(self, account):
        return sum(amount for owner, amount in self.entries if owner == account)


def transfer(ledger, sender, receiver, amount):
    if ledger.balance(sender) < amount:
        return False
    ledger.credit(sender, -amount)
    ledger.credit(receiver, amount)
    return True
