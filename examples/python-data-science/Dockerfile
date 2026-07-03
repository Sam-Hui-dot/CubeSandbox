# syntax=docker/dockerfile:1.7
#
# A Cube-ready sandbox image preconfigured for Python Data Science & AI Code Interpretation.
# It includes:
#   - Python3 & pip
#   - Core science stack: numpy, pandas, matplotlib, seaborn, scipy
#   - Chinese fonts (WenQuanYi Zen Hei) to prevent Matplotlib rendering square blocks for Chinese characters
#
# Build:
#   docker build -t cubesandbox-data-science:latest examples/python-data-science
#

ARG CUBE_BASE_IMAGE=ghcr.io/tencentcloud/cubesandbox-base:latest
FROM ${CUBE_BASE_IMAGE}

ARG DEBIAN_FRONTEND=noninteractive

# Install core packages, Python build tools, and Chinese fonts
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3-pip \
    python3-dev \
    build-essential \
    libfreetype6-dev \
    libpng-dev \
    fonts-wqy-zenhei \
    && rm -rf /var/lib/apt/lists/*

# Upgrade pip and install standard data science suite
RUN pip3 install --no-cache-dir --upgrade pip && \
    pip3 install --no-cache-dir \
    numpy \
    pandas \
    matplotlib \
    seaborn \
    scipy \
    tabulate

EXPOSE 49983
