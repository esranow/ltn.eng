# A real second Docker host for remote_e2e.py: Docker daemon (dind) behind sshd.
# Run with --privileged. The public key is mounted at /authorized_keys at run time.
FROM docker:dind
RUN apk add --no-cache openssh-server && ssh-keygen -A \
 && sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin prohibit-password/; s/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config \
 && passwd -u root 2>/dev/null || true
COPY entry.sh /entry.sh
RUN chmod +x /entry.sh
ENTRYPOINT ["/entry.sh"]
