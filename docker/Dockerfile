FROM ubuntu:20.04 as builder

ENV DEBIAN_FRONTEND noninteractive
RUN DEBIAN_FRONTEND=noninteractive apt-get update
RUN DEBIAN_FRONTEND=noninteractive apt install -y build-essential libstdc++-10-dev clang-10 libssl-dev zlib1g-dev cmake bsdmainutils git
# the makefile requires clang++
RUN ln -s /usr/bin/clang++-10 /usr/bin/clang++

RUN git clone --recursive https://github.com/rui314/mold.git

WORKDIR mold
RUN make submodules -j$(nproc)
RUN make STATIC=1 -j$(nproc)

FROM debian:buster-slim
COPY --from=builder /mold/mold /usr/local/bin/mold
ENTRYPOINT ["mold"]

