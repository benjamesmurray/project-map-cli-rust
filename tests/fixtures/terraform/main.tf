resource "aws_instance" "web" {
  ami           = "ami-0c55b159cbfafe1f0"
  instance_type = "t2.micro"
}

variable "region" {
  default = "us-west-2"
}

module "network" {
  source = "./modules/network"
}
