TESTS = $(wildcard *.sh)

test: $(TESTS)

$(TESTS):
	@./$@ `pwd`/../mold

.PHONY: test $(TESTS)
