class InvoiceService:
    def issue(self, amount):
        return amount

    def run(self):
        return self.issue(0)


class RetentionPolicy:
    def apply(self):
        return 30


def flush_buffer():
    pass
