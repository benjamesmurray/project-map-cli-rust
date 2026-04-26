import module_b
from module_a import main as run_main

class Service:
    def execute(self):
        module_b.hello()
        run_main()
