from service import Service

def compute(used_arg, dead_arg):
    return used_arg + 1

s = Service()
print(s.run(), compute(1, 2))
