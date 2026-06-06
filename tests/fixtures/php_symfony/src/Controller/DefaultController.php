<?php

namespace App\Controller;

use App\Service\MyService;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\Routing\Annotation\Route;

class DefaultController extends AbstractController
{
    /**
     * @Route("/", name="homepage")
     */
    #[Route('/hello', name: 'hello')]
    public function index(MyService $service)
    {
        return $this->render('base.html.twig');
    }
}
