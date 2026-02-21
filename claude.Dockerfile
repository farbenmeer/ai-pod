FROM ubuntu:latest


RUN apt-get update && apt-get install -y curl git

# Set up tools
#RUN claude mcp add --transport http figma-desktop http://host.containers.internal:3845/mcp
#RUN npm install -g @playwright/cli@latest
#RUN playwright-cli install --skills

WORKDIR /app

RUN useradd -ms /bin/bash claude
RUN chown -R claude /app
USER claude


ENV PATH="/home/claude/.local/bin:$PATH"
RUN curl -fsSL https://claude.ai/install.sh | bash


CMD ["claude"]
