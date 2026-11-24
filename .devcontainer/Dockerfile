ARG BASE
FROM ${BASE}

RUN if grep ubuntu:x:1000:1000 /etc/passwd >/dev/null; then userdel -f -r ubuntu; fi

ENV HISTSIZE=-1
ENV HISTFILESIZE=-1
ENV HISTFILE="/home/coder/.cache/._bash_history"

# only for the pylibfreenect2 container so it will run visual libraries
RUN export DEBIAN_FRONTEND=noninteractive \
 && apt update \
 && apt install --no-install-recommends -y \
    # From opengl/glvnd:runtime
    libxau6 libxdmcp6 libxcb1 libxext6 libx11-6 \
    libglvnd0 libopengl0 libgl1 libglx0 libegl1 libgles2 \
    # X11 dependencies
    libxrandr2 libxinerama1 libxcursor1 \
    # More X11 dependencies for opencv-python
    libsm6 libice6 \
    # Wayland dependencies
    wayland-protocols \
    libwayland-bin libwayland-egl1 libwayland-cursor0 \
    libwayland-client0 libwayland-server0 \
    libxkbcommon0 libxkbcommon-x11-0 \
    # GLEW dependencies
    libglvnd0 libgl1 libglx0 libegl1 libgles2 libglu1-mesa \
    # for libfreenect2
    libusb-1.0-0-dev libturbojpeg0-dev libglfw3-dev
