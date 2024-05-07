# Development instructions

> Happy when helming? Most likely not!

**NOTE:** All commands are relative to this file.

## Updating chart dependencies

```shell
helm dependency update charts/trustification-infrastructure
```

## Updating the JSON schema

The JSON schema must cover all values in the values file used by the Helm chart. The only exception are sections in
the values which are intended to be used by other charts, re-using the same values file. An example of this is the
`.global` section.

Unfortunately, Helm requires the JSON schema to be authored in JSON. To make that a little bit easier, we author it
in YAML and then convert it to JSON. For example, using:

```shell
python3 -c 'import sys, yaml, json; print(json.dumps(yaml.safe_load(sys.stdin), indent=2))' < charts/trustification/values.schema.yaml > charts/trustification/values.schema.json
```

## Linting Helm charts

```shell
helm lint ./charts/trustification --values values-minikube.yaml --set-string appDomain=.localhost
helm lint ./charts/trustification --values values-ocp-no-aws.yaml --set-string appDomain=.localhost
helm lint ./charts/trustification --values values-ocp-aws.yaml --values values-ocp-aws-lint.yaml --set-string appDomain=.localhost
```

Lint even more:

```shell
ct lint --charts ./charts/trustification  --helm-lint-extra-args "--values values-minikube.yaml --set-string appDomain=.localhost"
```

## Find that whitespace

```shell
helm template --debug charts/trustification
```

## Run the chart checks

> [!NOTE]
> This will only work when using OCP and having the AWS resources provisioned first.

```shell
podman run --rm \
    -e KUBECONFIG=/.kube/config \
    -v "${HOME}/.kube":/.kube:z \
    -v $(pwd):/charts:z \
    "quay.io/redhat-certification/chart-verifier:latest" \
    verify \
    -F /charts/values-ocp-aws.yaml -F /charts/values-ocp-aws-lint.yaml \
    /charts/charts/trustification
```

## Update the OpenShift templates

Helm charts are used to render the OpenShift templates in `../openshift`.

You can update those using:

```shell
make -C ../openshift
```

Read more in [../openshift/DEVELOPING.md](../openshift/DEVELOPING.md).
