SAMPLE_BASE_NAME = opencl_node_example
#TODO get the OpenCL paths from the NBTS system list
OPENCL_ROOT_DIR = /mnt/tbbusers/tbbtest/soft/INDE

ifeq ($(shell uname), Linux)
SOURCE_OPENCL = . $(OPENCL_ROOT_DIR)/opencl_vars.sh intel64 &&
OPENCL_LIB = -lOpenCL
endif

ifeq ($(shell uname), Darwin)
OPENCL_LIB = -framework OpenCL
endif

release:
	$(SOURCE_OPENCL) $(CXX) -O2 -o $(SAMPLE_BASE_NAME).$(OUTPUT_EXTENSION) $(CXXFLAGS) \
	                               $(SAMPLE_BASE_NAME).cpp -ltbb $(OPENCL_LIB) $(LIBS)
debug:
	$(SOURCE_OPENCL) $(CXX) -O0 -o $(SAMPLE_BASE_NAME).$(OUTPUT_EXTENSION) $(CXXFLAGS) \
	                               $(SAMPLE_BASE_NAME).cpp -ltbb_debug $(OPENCL_LIB) $(LIBS)

run:
	$(SOURCE_OPENCL) mv $(SAMPLE_BASE_NAME).$(OUTPUT_EXTENSION) $(PROG) && \
	./$(PROG)
