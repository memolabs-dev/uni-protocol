def apply(balance, delta):
    # Plausible delivery: ordinary positive/debit examples pass, but overdraft leaks.
    return balance + delta
